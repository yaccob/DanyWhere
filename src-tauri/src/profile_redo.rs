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
//! - speech model, languages, word lists or filler words: the recording is
//!   transcribed again, then post-processed as the target profile says;
//! - only post-processing: the transcription is post-processed again;
//! - only the "Append" field: the text stays, the target's "Append" applies.
//!
//! Only while choosing the profile by application is on; otherwise
//! everything stays as before. "Copy Last Transcript" has no target, so it
//! keeps the dictation's profile. A dictation the user changed in History
//! (an edit, or more versions than its own post-processing) is not redone:
//! the user's text wins, only its "Append" text follows the target.
//!
//! What a redo stores in History follows "Transcribe Again" and the
//! post-processing from History (`history_versions`): transcribing again
//! replaces the entry's stack, post-processing again replaces the
//! post-processing the dictation made, or puts a new version on top of a
//! transcription that had none.

use std::sync::{Arc, Mutex};

use log::{info, warn};
use tauri::{AppHandle, Manager};

use crate::history_versions::{HistoryVersion, PostProcessTarget, PostProcessed, VersionAuthor};
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::transcription::TranscriptionManager;
use crate::notices::{Notice, NoticeCode, NoticeSource};
use crate::profiles::ProfileValues;
use crate::settings::AppSettings;

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
/// model gets or finds (the active additional list counts, not the others)
/// or filler words.
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
        || words(a) != words(b)
}

/// The text of the prompt `values` post-process with.
fn selected_prompt(values: &ProfileValues) -> Option<&str> {
    let id = values.post_process_selected_prompt_id.as_deref()?;
    values
        .post_process_prompts
        .iter()
        .find(|prompt| prompt.id == id)
        .map(|prompt| prompt.prompt.as_str())
}

/// Whether post-processing with `a` can come out other than with `b`, both
/// post-processing: another provider, the provider's model or effort,
/// another prompt text, or other context.
fn post_processing_differs(a: &ProfileValues, b: &ProfileValues) -> bool {
    let provider = &a.post_process_provider_id;
    let context_limits = |values: &ProfileValues| {
        (
            values.context_age_limit_minutes,
            values.context_max_entries,
            values.context_char_budget,
        )
    };
    a.post_process_provider_id != b.post_process_provider_id
        || a.post_process_models.get(provider) != b.post_process_models.get(provider)
        || a.post_process_efforts.get(provider) != b.post_process_efforts.get(provider)
        || selected_prompt(a) != selected_prompt(b)
        || a.send_context != b.send_context
        || (a.send_context && context_limits(a) != context_limits(b))
}

/// The first step to redo when a dictation made with `made_with` goes into
/// an application whose profile has `target`; `post_process_requested` is
/// whether the dictation's shortcut asked for post-processing.
pub fn redo_from(
    base: &AppSettings,
    made_with: &ProfileValues,
    target: &ProfileValues,
    post_process_requested: bool,
) -> RedoFrom {
    if transcription_differs(base, made_with, target) {
        return RedoFrom::Transcription;
    }
    let made_post_processed = post_process_requested && made_with.post_process_enabled;
    let target_post_processes = post_process_requested && target.post_process_enabled;
    if made_post_processed != target_post_processes
        || (made_post_processed && post_processing_differs(made_with, target))
    {
        return RedoFrom::PostProcessing;
    }
    if made_with.append_text != target.append_text {
        return RedoFrom::Append;
    }
    RedoFrom::Nothing
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

/// Whether the entry's versions are still those its dictation made: the
/// transcription alone, or with one post-processing on it.
pub fn redoable(versions: &[HistoryVersion]) -> bool {
    match versions {
        [transcription] => !edited_by_user(transcription),
        [_, top] => {
            top.steps
                .first()
                .is_some_and(|step| step.by == VersionAuthor::Ai)
                && !edited_by_user(top)
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
        (true, true) => RedoPlan::PostProcess(PostProcessTarget::Redo),
        (true, false) => RedoPlan::PostProcess(PostProcessTarget::New),
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
    /// Whether its shortcut asked for post-processing.
    pub post_process_requested: bool,
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
    /// The target profile's values, for its "Append" field.
    pub values: ProfileValues,
    /// A redone post-processing failed; the text is the transcription.
    pub post_process_failed: bool,
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
            values: target_values,
            post_process_failed: false,
        });
    };
    let from = redo_from(
        &settings,
        &made.values,
        &target_values,
        made.post_process_requested,
    );
    let target_post_processes = made.post_process_requested && target_values.post_process_enabled;
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
    let (text, post_process_failed) =
        run(app, entry, &made, &target_id, &target_values, plan, show);
    Some(ForInsert {
        text,
        values: target_values,
        post_process_failed,
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
    if insert.post_process_failed {
        let notice = Notice::new(NoticeCode::PostProcessFailed, NoticeSource::Dictation);
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
/// the new text, if any, and whether its post-processing failed.
fn run(
    app: &AppHandle,
    entry: &HistoryEntry,
    made: &MadeWith,
    target_id: &str,
    target: &ProfileValues,
    plan: RedoPlan,
    show: &dyn Fn(Working),
) -> (Option<String>, bool) {
    let redone = |requested: bool| MadeWith {
        entry_id: entry.id,
        profile_id: target_id.to_string(),
        values: target.clone(),
        post_process_requested: requested,
    };
    match plan {
        RedoPlan::Keep => (None, false),
        RedoPlan::UseTranscription => (
            entry.versions.first().map(|version| version.text.clone()),
            false,
        ),
        RedoPlan::PostProcess(where_to) => {
            show(Working::PostProcessing);
            let transcription = entry
                .versions
                .first()
                .map_or(entry.transcription_text.as_str(), |version| {
                    version.text.as_str()
                });
            match crate::app_profiles::with_values_here(target.clone(), || {
                post_process_again(app, entry, transcription, where_to)
            }) {
                Ok(text) => {
                    remember(redone(made.post_process_requested));
                    (Some(text), false)
                }
                Err(e) => {
                    warn!(
                        "Post-processing entry {} for profile {target_id} failed: {e}",
                        entry.id
                    );
                    (Some(transcription.to_string()), true)
                }
            }
        }
        RedoPlan::Transcribe => {
            show(Working::Transcribing);
            let post_process = made.post_process_requested && target.post_process_enabled;
            match crate::app_profiles::with_values_here(target.clone(), || {
                transcribe_again(app, entry, post_process, show)
            }) {
                Ok((text, failed)) => {
                    remember(redone(made.post_process_requested));
                    (Some(text), failed)
                }
                Err(e) => {
                    warn!(
                        "Transcribing entry {} for profile {target_id} failed: {e}",
                        entry.id
                    );
                    (None, false)
                }
            }
        }
    }
}

/// Post-processes `transcription` with this thread's profile values and
/// stores the result as `where_to` says. Errors when no text came out.
fn post_process_again(
    app: &AppHandle,
    entry: &HistoryEntry,
    transcription: &str,
    where_to: PostProcessTarget,
) -> anyhow::Result<String> {
    let settings = crate::app_profiles::dictation_settings(app);
    let with_context = crate::send_context::sends_context(true, &settings);
    // The word list's hint goes along with the transcription it was made in.
    let mut query = crate::dictation_context::ContextQuery::for_entry(
        entry,
        entry.window_context.word_replacements.clone(),
    );
    if !with_context {
        query.window = None;
    }
    let processed = tauri::async_runtime::block_on(crate::actions::process_transcription_output(
        app,
        transcription,
        true,
        &query,
    ));
    let text = processed
        .post_processed_text
        .ok_or_else(|| anyhow::anyhow!("no post-processed text"))?;
    let hm = app.state::<Arc<HistoryManager>>();
    hm.store_post_processing(
        entry.id,
        where_to,
        PostProcessed {
            text: text.clone(),
            prompt: processed.post_process_prompt,
            prompt_name: processed.post_process_prompt_name,
        },
    )?;
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
/// `post_process`, and stores both as "Transcribe Again" does. Returns the
/// text and whether the post-processing failed.
fn transcribe_again(
    app: &AppHandle,
    entry: &HistoryEntry,
    post_process: bool,
    show: &dyn Fn(Working),
) -> anyhow::Result<(String, bool)> {
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
    let settings = crate::app_profiles::dictation_settings(app);
    let with_context = crate::send_context::sends_context(post_process, &settings);
    let mut query = crate::dictation_context::ContextQuery::for_entry(
        entry,
        transcription.word_replacements.clone(),
    );
    if !with_context {
        query.window = None;
    }
    let processed = tauri::async_runtime::block_on(crate::actions::process_transcription_output(
        app,
        &transcription.text,
        post_process,
        &query,
    ));
    hm.update_transcription(
        entry.id,
        transcription.text,
        processed.post_processed_text,
        processed.post_process_prompt,
        transcription.language,
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
    Ok((processed.final_text, processed.post_process_failed))
}

/// Whether a version was made by the user, in any step.
fn edited_by_user(version: &HistoryVersion) -> bool {
    version
        .steps
        .iter()
        .any(|step| step.by == VersionAuthor::User)
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

    /// The default profile (active) and "mail", a copy of it assigned to
    /// Mail; tests change mail's values.
    fn settings_with_mail() -> AppSettings {
        let mut settings = AppSettings {
            selected_model: "parakeet".to_string(),
            post_process_enabled: true,
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
            post_process_selected_prompt_id: Some("clean".to_string()),
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
            append_text: " ".to_string(),
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

    fn redo(change: impl FnOnce(&mut ProfileValues), requested: bool) -> RedoFrom {
        let settings = settings_with_mail();
        let made = default_values(&settings);
        let mut target = made.clone();
        change(&mut target);
        redo_from(&settings, &made, &target, requested)
    }

    #[test]
    fn the_same_values_redo_nothing() {
        assert_eq!(redo(|_| {}, true), RedoFrom::Nothing);
        assert_eq!(redo(|_| {}, false), RedoFrom::Nothing);
    }

    #[test]
    fn another_append_text_only_swaps_it() {
        assert_eq!(
            redo(|t| t.append_text = "\\n{Enter}".to_string(), true),
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
                    t.post_process_selected_prompt_id = Some("formal".to_string());
                    t.append_text = String::new();
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

    #[test]
    fn another_prompt_post_processes_again_while_post_processing() {
        let formal =
            |t: &mut ProfileValues| t.post_process_selected_prompt_id = Some("formal".to_string());
        assert_eq!(redo(formal, true), RedoFrom::PostProcessing);
        // Without post-processing the prompt plays no part.
        assert_eq!(redo(formal, false), RedoFrom::Nothing);
        // The same prompt text under another id is the same.
        assert_eq!(
            redo(
                |t| {
                    t.post_process_prompts[1].prompt = "Clean it up.".to_string();
                    t.post_process_selected_prompt_id = Some("formal".to_string());
                },
                true
            ),
            RedoFrom::Nothing
        );
    }

    #[test]
    fn switching_post_processing_on_or_off_post_processes_again() {
        assert_eq!(
            redo(|t| t.post_process_enabled = false, true),
            RedoFrom::PostProcessing
        );
        // The shortcut didn't ask for it: neither post-processes.
        assert_eq!(
            redo(|t| t.post_process_enabled = false, false),
            RedoFrom::Nothing
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
            redo(|t| t.send_context = !t.send_context, true),
            RedoFrom::PostProcessing
        );
        assert_eq!(
            redo(|t| t.send_context = !t.send_context, false),
            RedoFrom::Nothing
        );
    }

    #[test]
    fn the_target_is_the_profile_of_the_app_in_front_while_choosing_by_app() {
        let mut settings = settings_with_mail();
        settings.profiles[0].values.append_text = "\\n".to_string();
        let (id, values) = target_for(&settings, Some("com.apple.mail")).expect("mail");
        assert_eq!(id, "mail");
        assert_eq!(values.append_text, "\\n");
        // An app without a profile gets the defaults in use, with their
        // latest values from the flat fields.
        settings.append_text = "  ".to_string();
        let (id, values) = target_for(&settings, Some("com.apple.TextEdit")).expect("default");
        assert_eq!(id, DEFAULT_PROFILE_ID);
        assert_eq!(values.append_text, "  ");
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
            RedoPlan::PostProcess(PostProcessTarget::Redo)
        );
        let bare = entry(vec![transcription("a")]);
        assert_eq!(
            plan(RedoFrom::PostProcessing, &bare, true),
            RedoPlan::PostProcess(PostProcessTarget::New)
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
            RedoPlan::PostProcess(PostProcessTarget::Redo)
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
            post_process_requested: true,
        };
        remember(made(41));
        remember(made(42));
        assert_eq!(made_with(42), Some(made(42)));
        assert_eq!(made_with(41), None);
    }
}
