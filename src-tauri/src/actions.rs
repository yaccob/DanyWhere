#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::audio_feedback::{play_feedback_sound, SoundType};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::StreamWorkKind;
use crate::managers::transcription::{Transcription, TranscriptionManager};
use crate::notices::{self, Notice, NoticeCode, NoticeSource};
use crate::settings::{AppSettings, OverlayStyle, APPLE_INTELLIGENCE_PROVIDER_ID};
use crate::shortcut;
use crate::tray::{set_tray_state, TrayIconState};
use crate::utils::{self, show_processing_overlay, show_transcribing_overlay};
use crate::TranscriptionCoordinator;
use log::{debug, error, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

/// How a dictation ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionOutcome {
    /// The text was typed.
    Typed,
    /// Nothing was heard, so nothing was typed.
    Empty,
    /// The microphone sent only digital silence (e.g. muted).
    Silent,
    /// Transcribing or pasting failed.
    Failed,
}

/// Emitted when a dictation finishes (not on cancel); onboarding uses it to measure speed.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct TranscriptionCompleteEvent {
    pub outcome: TranscriptionOutcome,
    pub audio_seconds: f64,
    pub wait_seconds: f64,
}

fn emit_transcription_complete(
    app: &AppHandle,
    outcome: TranscriptionOutcome,
    sample_count: usize,
    stop_time: Instant,
) {
    let event = TranscriptionCompleteEvent {
        outcome,
        audio_seconds: sample_count as f64
            / crate::audio_toolkit::constants::WHISPER_SAMPLE_RATE as f64,
        wait_seconds: stop_time.elapsed().as_secs_f64(),
    };
    if let Err(e) = event.emit(app) {
        warn!("Failed to emit transcription complete: {e}");
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

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Bumped per recording session, so a late paste from an earlier session
/// doesn't reset the tray or hide the overlay of a newer one.
static RECORDING_SESSION: AtomicU64 = AtomicU64::new(0);

/// DanyWhere: the current recording session, for `dictation_key_inserts`.
pub fn current_recording_session() -> u64 {
    RECORDING_SESSION.load(Ordering::SeqCst)
}

/// DanyWhere: a new recording session begins (`dictation_start`).
pub(crate) fn begin_recording_session() {
    RECORDING_SESSION.fetch_add(1, Ordering::SeqCst);
}

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

// Shortcut Action Trait
pub trait ShortcutAction: Send + Sync {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
}

// Transcribe Action. DanyWhere: the same for both start shortcuts; what
// follows the recording is decided by how it ends (`end_keys`).
struct TranscribeAction;

/// Field name for structured output JSON schema
const TRANSCRIPTION_FIELD: &str = "transcription";

/// Strip invisible Unicode characters that some LLMs may insert
fn strip_invisible_chars(s: &str) -> String {
    s.replace(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'], "")
}

/// Strip a leading `<think>...</think>` block. Some endpoints can't disable
/// reasoning, and some local servers put the reasoning text into `content`
/// instead of a separate field — without this the user would get the model's
/// chain of thought pasted along with the cleaned transcription.
fn strip_think_block(s: &str) -> &str {
    if let Some(rest) = s.trim_start().strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            return rest[end + "</think>".len()..].trim_start();
        }
    }
    s
}

/// Build a system prompt from the user's prompt template.
/// Removes `${output}` placeholder since the transcription is sent as the user message.
fn build_system_prompt(prompt_template: &str) -> String {
    prompt_template.replace("${output}", "").trim().to_string()
}

/// DanyWhere: the system prompt and the user's message for a provider with
/// structured output. A prompt with `${output}` goes as before; any other
/// gets the fixed frame, and the transcript comes in tags (`prompt_frame`).
fn structured_request(
    prompt: &str,
    context: Option<&str>,
    transcription: &str,
) -> (String, String) {
    use crate::prompt_frame;
    if prompt_frame::uses_output_placeholder(prompt) {
        return (
            build_system_prompt(&crate::dictation_context::prompt_with_context(
                prompt, context,
            )),
            transcription.to_string(),
        );
    }
    (
        prompt_frame::framed_instructions(prompt, context),
        prompt_frame::tagged_transcript(transcription),
    )
}

/// DanyWhere: the one message for a provider without structured output. A
/// prompt with `${output}` gets the transcript in its place, as before; any
/// other gets the frame, and the transcript in tags at the end. The context
/// block goes after the filled-in prompt, so a context text can never be
/// taken for the placeholder.
fn single_message_request(prompt: &str, context: Option<&str>, transcription: &str) -> String {
    use crate::prompt_frame;
    if prompt_frame::uses_output_placeholder(prompt) {
        return crate::dictation_context::prompt_with_context(
            &prompt.replace(prompt_frame::OUTPUT_PLACEHOLDER, transcription),
            context,
        );
    }
    prompt_frame::framed_message(prompt, context, transcription)
}

/// Returns `true` when a transcription has no meaningful content to
/// post-process (empty or whitespace-only). Used to skip the post-processing
/// LLM call when nothing was actually transcribed, which would otherwise make
/// the model reply with an error message such as "you need to provide the
/// transcription".
fn is_blank_transcription(transcription: &str) -> bool {
    transcription.trim().is_empty()
}

fn non_blank_post_processed_text(text: String) -> Option<String> {
    if is_blank_transcription(&text) {
        warn!(
            "Post-processing returned an empty response. Falling back to original transcription."
        );
        None
    } else {
        Some(text)
    }
}

async fn complete_unless_cancelled<F, C>(operation: F, is_cancelled: C) -> Option<F::Output>
where
    F: Future,
    C: Fn() -> bool,
{
    tokio::pin!(operation);

    loop {
        if is_cancelled() {
            return None;
        }

        if let Ok(result) =
            tokio::time::timeout(CANCELLATION_POLL_INTERVAL, operation.as_mut()).await
        {
            return Some(result);
        }
    }
}

fn should_use_streaming_overlay(style: OverlayStyle, is_streaming: bool) -> bool {
    style == OverlayStyle::Live && is_streaming
}

/// What became of a transcription's post-processing.
#[derive(Debug, PartialEq, Eq)]
enum PostProcessOutcome {
    /// There was no speech to process.
    Skipped,
    Done(String),
    /// The user asked for it, but it couldn't run or produced nothing, so the
    /// transcript goes out as it is.
    Failed,
}

/// DanyWhere: a post-processing outcome, and the model that refused the
/// profile's effort (`post_process_effort`), so the text came at its default.
#[derive(Debug, PartialEq, Eq)]
struct PostProcessReport {
    outcome: PostProcessOutcome,
    effort_refused_by: Option<String>,
}

impl From<PostProcessOutcome> for PostProcessReport {
    fn from(outcome: PostProcessOutcome) -> Self {
        Self {
            outcome,
            effort_refused_by: None,
        }
    }
}

/// DanyWhere: [`post_process_transcription`] under the time limit from the
/// settings (Handy: a fixed 60 s). On timeout it falls back like any other
/// failure, so the original transcription goes out.
async fn post_process_within_time_limit(
    settings: &AppSettings,
    transcription: &str,
    context: Option<&crate::dictation_context::ContextBlock>,
) -> PostProcessReport {
    let limit = crate::llm_timeouts::post_process_timeout(settings);
    match tokio::time::timeout(
        limit,
        post_process_transcription(settings, transcription, context),
    )
    .await
    {
        Ok(report) => report,
        Err(_) => {
            warn!(
                "Post-processing timed out after {:?}; pasting the original transcription",
                limit
            );
            PostProcessOutcome::Failed.into()
        }
    }
}

/// DanyWhere: `context` (earlier dictations from the same window, see
/// `dictation_context`) goes after the active prompts, in every path; the
/// report also says whether the profile's effort was refused.
async fn post_process_transcription(
    settings: &AppSettings,
    transcription: &str,
    context: Option<&crate::dictation_context::ContextBlock>,
) -> PostProcessReport {
    let mut effort_refused = false;
    let outcome = post_process_outcome(settings, transcription, context, &mut effort_refused).await;
    let model = settings
        .active_post_process_provider()
        .and_then(|provider| settings.post_process_models.get(&provider.id))
        .cloned()
        .unwrap_or_default();
    PostProcessReport {
        effort_refused_by: (effort_refused && matches!(outcome, PostProcessOutcome::Done(_)))
            .then_some(model),
        outcome,
    }
}

/// Handy's post-processing. DanyWhere: sets `effort_refused` when the reply
/// came without the profile's effort (`llm_client::Completion`).
async fn post_process_outcome(
    settings: &AppSettings,
    transcription: &str,
    context: Option<&crate::dictation_context::ContextBlock>,
    effort_refused: &mut bool,
) -> PostProcessOutcome {
    if is_blank_transcription(transcription) {
        debug!("Post-processing skipped because the transcription is empty");
        return PostProcessOutcome::Skipped;
    }

    let provider = match settings.active_post_process_provider().cloned() {
        Some(provider) => provider,
        None => {
            debug!("Post-processing enabled but no provider is selected");
            return PostProcessOutcome::Failed;
        }
    };

    let model = settings
        .post_process_models
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    if model.trim().is_empty() {
        debug!(
            "Post-processing skipped because provider '{}' has no model configured",
            provider.id
        );
        return PostProcessOutcome::Failed;
    }

    // DanyWhere: the profile's active prompts from the shared library, all
    // in one request (`prompt_library`), instead of Handy's selected prompt.
    let prompts = crate::prompt_library::requested(settings);
    if prompts.is_empty() {
        debug!("Post-processing skipped because no prompt with a text is active");
        return PostProcessOutcome::Failed;
    }
    let prompt = crate::prompt_library::request_instructions(&prompts);
    let prompt = crate::prompt_frame::with_known_terms(&prompt, settings);
    log::info!(
        "Post-processing with {}",
        crate::prompt_library::request_names(&prompts).unwrap_or_default()
    );

    debug!(
        "Starting LLM post-processing with provider '{}' (model: {})",
        provider.id, model
    );
    // DanyWhere: which way the prompt goes (`prompt_frame`).
    if crate::prompt_frame::uses_output_placeholder(&prompt) {
        log::info!("Post-processing without the fixed frame: the prompt has ${{output}}");
    } else {
        log::info!("Post-processing with the fixed frame");
    }

    let api_key = settings
        .post_process_api_keys
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    // Ask these providers to skip reasoning/thinking — post-processing rarely
    // benefits from it and it adds seconds of latency. llm_client picks the
    // field the endpoint understands and retries without it if rejected.
    // DanyWhere: an effort chosen for the profile wins over this.
    let reasoning = crate::llm_client::ReasoningRequest {
        disable_by_default: matches!(provider.id.as_str(), "custom" | "openrouter"),
        effort: crate::post_process_effort::for_request(settings, &provider, &model),
    };
    if let Some(effort) = &reasoning.effort {
        log::info!("Post-processing with effort '{effort}'");
    }

    if provider.supports_structured_output {
        debug!("Using structured outputs for provider '{}'", provider.id);

        // DanyWhere: the frame and the context block (`prompt_frame`,
        // `dictation_context`).
        let (system_prompt, user_content) =
            structured_request(&prompt, context.map(|c| c.system.as_str()), transcription);

        // Handle Apple Intelligence separately since it uses native Swift APIs
        if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            {
                if !apple_intelligence::check_apple_intelligence_availability() {
                    debug!(
                        "Apple Intelligence selected but not currently available on this device"
                    );
                    return PostProcessOutcome::Failed;
                }

                let token_limit = model.trim().parse::<i32>().unwrap_or(0);
                return match apple_intelligence::process_text_with_system_prompt(
                    &system_prompt,
                    &user_content,
                    token_limit,
                ) {
                    Ok(result) => {
                        if result.trim().is_empty() {
                            debug!("Apple Intelligence returned an empty response");
                            PostProcessOutcome::Failed
                        } else {
                            let result = strip_invisible_chars(&result);
                            debug!(
                                "Apple Intelligence post-processing succeeded. Output length: {} chars",
                                result.len()
                            );
                            PostProcessOutcome::Done(result)
                        }
                    }
                    Err(err) => {
                        error!("Apple Intelligence post-processing failed: {}", err);
                        PostProcessOutcome::Failed
                    }
                };
            }

            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            {
                debug!("Apple Intelligence provider selected on unsupported platform");
                return PostProcessOutcome::Failed;
            }
        }

        // Define JSON schema for transcription output
        let json_schema = serde_json::json!({
            "type": "object",
            "properties": {
                (TRANSCRIPTION_FIELD): {
                    "type": "string",
                    "description": "The cleaned and processed transcription text"
                }
            },
            "required": [TRANSCRIPTION_FIELD],
            "additionalProperties": false
        });

        match crate::llm_client::send_chat_completion_with_schema(
            &provider,
            api_key.clone(),
            &model,
            user_content,
            Some(system_prompt),
            Some(json_schema),
            &reasoning,
        )
        .await
        {
            Ok(crate::llm_client::Completion {
                content: Some(content),
                effort_ignored,
            }) => {
                *effort_refused = effort_ignored;
                // Parse the JSON response to extract the transcription field
                let content = strip_think_block(&content);
                match serde_json::from_str::<serde_json::Value>(content) {
                    Ok(json) => {
                        if let Some(transcription_value) =
                            json.get(TRANSCRIPTION_FIELD).and_then(|t| t.as_str())
                        {
                            let result = strip_invisible_chars(transcription_value);
                            debug!(
                                "Structured output post-processing succeeded for provider '{}'. Output length: {} chars",
                                provider.id,
                                result.len()
                            );
                            return PostProcessOutcome::Done(result);
                        } else {
                            error!("Structured output response missing 'transcription' field");
                            return PostProcessOutcome::Done(strip_invisible_chars(content));
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to parse structured output JSON: {}. Returning raw content.",
                            e
                        );
                        return PostProcessOutcome::Done(strip_invisible_chars(content));
                    }
                }
            }
            Ok(crate::llm_client::Completion { content: None, .. }) => {
                error!("LLM API response has no content");
                return PostProcessOutcome::Failed;
            }
            Err(e) => {
                warn!(
                    "Structured output failed for provider '{}': {}. Falling back to legacy mode.",
                    provider.id, e
                );
                // Fall through to legacy mode below
            }
        }
    }

    // Legacy mode: Replace ${output} variable in the prompt with the actual text
    // DanyWhere: or the frame, see `single_message_request`.
    let processed_prompt =
        single_message_request(&prompt, context.map(|c| c.legacy.as_str()), transcription);
    debug!("Processed prompt length: {} chars", processed_prompt.len());

    match crate::llm_client::send_chat_completion(
        &provider,
        api_key,
        &model,
        processed_prompt,
        &reasoning,
    )
    .await
    {
        Ok(crate::llm_client::Completion {
            content: Some(content),
            effort_ignored,
        }) => {
            *effort_refused = effort_ignored;
            let content = strip_invisible_chars(strip_think_block(&content));
            debug!(
                "LLM post-processing succeeded for provider '{}'. Output length: {} chars",
                provider.id,
                content.len()
            );
            PostProcessOutcome::Done(content)
        }
        Ok(crate::llm_client::Completion { content: None, .. }) => {
            error!("LLM API response has no content");
            PostProcessOutcome::Failed
        }
        Err(e) => {
            error!(
                "LLM post-processing failed for provider '{}': {}. Falling back to original transcription.",
                provider.id,
                e
            );
            PostProcessOutcome::Failed
        }
    }
}

pub(crate) struct ProcessedTranscription {
    pub final_text: String,
    pub post_processed_text: Option<String>,
    pub post_process_prompt: Option<String>,
    /// DanyWhere: the name of that prompt, for History's versions
    /// (`history_versions`).
    pub post_process_prompt_name: Option<String>,
    /// `final_text` is the bare transcript because post-processing failed.
    pub post_process_failed: bool,
    /// DanyWhere: the model that refused the profile's effort, so the text
    /// came at its default (`post_process_effort`).
    pub post_process_effort_refused_by: Option<String>,
    /// DanyWhere: the earlier entries sent along as context
    /// (`dictation_context`).
    pub context_entry_ids: Vec<i64>,
}

/// DanyWhere: `context` says which earlier dictations may go along as
/// context for post-processing (`dictation_context`).
pub(crate) async fn process_transcription_output(
    app: &AppHandle,
    transcription: &str,
    post_process: bool,
    context: &crate::dictation_context::ContextQuery,
) -> ProcessedTranscription {
    process_transcription_output_with(app, transcription, post_process, context, None).await
}

/// DanyWhere: [`process_transcription_output`]; with `prompt_ids` it
/// post-processes with exactly those prompts of the library, in that order,
/// instead of the profile's active ones (History's wand menu,
/// `prompt_library::with_only`).
pub(crate) async fn process_transcription_output_with(
    app: &AppHandle,
    transcription: &str,
    post_process: bool,
    context: &crate::dictation_context::ContextQuery,
    prompt_ids: Option<&[String]>,
) -> ProcessedTranscription {
    // DanyWhere: with the values of the dictation's own profile, if any.
    let mut settings = crate::app_profiles::dictation_settings(app);
    if let Some(ids) = prompt_ids {
        settings.post_process_prompt_order = crate::prompt_library::with_only(
            &settings.post_process_prompt_order,
            &settings.post_process_prompts,
            ids,
        );
    }
    let mut final_text = transcription.to_string();
    let mut post_processed_text: Option<String> = None;
    let mut post_process_prompt: Option<String> = None;
    let mut post_process_prompt_name: Option<String> = None;
    let mut post_process_failed = false;
    let mut post_process_effort_refused_by: Option<String> = None;
    let mut context_entry_ids: Vec<i64> = Vec::new();

    if post_process {
        // DanyWhere: the active prompts' instructions and names
        // (`prompt_library`), and earlier dictations from the same window,
        // if "Send Context" asked for them (the query then has a window).
        let (instructions, prompt_names) = {
            let prompts = crate::prompt_library::requested(&settings);
            (
                crate::prompt_library::request_instructions(&prompts),
                crate::prompt_library::request_names(&prompts),
            )
        };
        let context_block = if is_blank_transcription(&final_text) {
            None
        } else {
            crate::dictation_context::context_for(app, &settings, &instructions, context)
        };
        // A reply that's blank once cleaned is no better than none.
        let report =
            post_process_within_time_limit(&settings, &final_text, context_block.as_ref()).await;
        let outcome = match report.outcome {
            PostProcessOutcome::Done(text) => non_blank_post_processed_text(text)
                .map_or(PostProcessOutcome::Failed, PostProcessOutcome::Done),
            outcome => outcome,
        };
        if matches!(outcome, PostProcessOutcome::Done(_)) {
            post_process_effort_refused_by = report.effort_refused_by;
        }
        // The request was made, so the context left the computer.
        if outcome != PostProcessOutcome::Skipped {
            if let Some(block) = &context_block {
                context_entry_ids = block.entry_ids.clone();
            }
        }
        match outcome {
            PostProcessOutcome::Skipped => {}
            PostProcessOutcome::Failed => post_process_failed = true,
            PostProcessOutcome::Done(processed_text) => {
                post_processed_text = Some(processed_text.clone());
                final_text = processed_text;
                // DanyWhere: what the request carried, and the names of all
                // its prompts for History's step.
                post_process_prompt = Some(instructions);
                post_process_prompt_name = prompt_names;
            }
        }
    }

    ProcessedTranscription {
        final_text,
        post_processed_text,
        post_process_prompt,
        post_process_prompt_name,
        post_process_failed,
        post_process_effort_refused_by,
        context_entry_ids,
    }
}

/// DanyWhere: looks at a finished dictation (`entry_id`, the History entry
/// it was saved as) going into the application in front right away, pasted
/// or inserted by the service call waiting for it, and redoes it for that
/// application's profile when needed (`profile_redo::for_insert`). `None`:
/// it goes as it is, with its own profile's values; as when it is copied
/// ("Copy Last Transcript" during the dictation) or waits to be inserted.
fn redo_for_app_in_front(
    app: &AppHandle,
    hm: &HistoryManager,
    entry_id: Option<i64>,
    session: u64,
    show: &dyn Fn(crate::profile_redo::Working),
) -> Option<crate::profile_redo::ForInsert> {
    let entry_id = entry_id?;
    if crate::dictation_key_inserts::copies_instead_of_pasting(session)
        || !crate::delivery::goes_into_app_in_front(
            // DanyWhere: the dictation's paste method (this thread's values).
            &crate::app_profiles::dictation_settings(app),
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

impl ShortcutAction for TranscribeAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        // DanyWhere: decided and done through ports (`dictation_start`), so
        // tests run it without the app.
        crate::dictation_start::app_ports::start(app, binding_id);
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        stop_transcription(
            app,
            binding_id,
            shortcut_str,
            &crate::end_keys::EndChoice::Start(binding_id.to_string()),
        );
    }
}

/// Stops the recording of `binding_id` and finishes the dictation as `end`
/// says (DanyWhere, `end_keys`): its end shortcut's append text,
/// post-processing (as far as the dictation's profile allows it) and
/// context. The transcription coordinator calls this for every end.
pub fn stop_transcription(
    app: &AppHandle,
    binding_id: &str,
    _shortcut_str: &str,
    end: &crate::end_keys::EndChoice,
) {
    // Handy's `TranscribeAction::stop`, left indented as it was so merges
    // from Handy stay small.
    {
        // Prevent a slow microphone from emitting a ready event or start chime
        // after the user has already requested stop.
        app.state::<Arc<AudioRecordingManager>>()
            .invalidate_recording_readiness();

        // Unregister the cancel shortcut when transcription stops
        shortcut::unregister_cancel_shortcut(app);

        let stop_time = Instant::now();
        debug!("TranscribeAction::stop called for binding: {}", binding_id);

        let ah = app.clone();
        let rm = Arc::clone(&app.state::<Arc<AudioRecordingManager>>());
        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
        let hm = Arc::clone(&app.state::<Arc<HistoryManager>>());

        // DanyWhere: the dictation's profile is chosen again now, by the
        // application in front when the recording ends, which is where its
        // text goes (user, 2026-11-16; `app_profiles::settle_dictation`).
        // The pills name it from here on. A live decode made with the
        // start's values no longer fits: the recording is transcribed in one
        // go with the end's values, its model loading meanwhile.
        let transcribed_with_end_values = match crate::app_profiles::settle_dictation(app) {
            Some(change) => {
                crate::overlay::set_dictation_profile(app, change.shown);
                if change.transcription_differs {
                    tm.cancel_stream();
                    tm.initiate_model_load(true);
                }
                change.transcription_differs
            }
            None => false,
        };

        set_tray_state(app, TrayIconState::Transcribing);
        // Stop should give immediate visual feedback. Live streaming can keep
        // the larger panel, but it still switches from listening to a working
        // spinner while the stream finalizes. Non-streaming paths use the
        // compact transcribing pill (None no-ops in show_*).
        // DanyWhere: the dictation's profile's style.
        let style = crate::app_profiles::dictation_settings(app).overlay_style;
        // Capture this before finalizing the stream so every later working state
        // targets the same overlay that was shown for this transcription.
        let use_streaming_overlay = should_use_streaming_overlay(style, tm.is_streaming());
        if use_streaming_overlay {
            tm.emit_stream_working(StreamWorkKind::Transcribing);
        } else {
            show_transcribing_overlay(app);
        }

        // Unmute before playing audio feedback so the stop sound is audible
        rm.remove_mute();

        // Play audio feedback for recording stop
        play_feedback_sound(app, SoundType::Stop);

        let binding_id = binding_id.to_string(); // Clone binding_id for the async task

        // DanyWhere: what it is made with, for redoing it when its text goes
        // into an application with another profile (`profile_redo`).
        let (profile_id, profile_values) = crate::app_profiles::dictation_profile_and_values(app);
        // DanyWhere: the end shortcut it ended with, in the profile it ends
        // with (`end_keys`): its append text, whether it post-processes and
        // whether earlier dictations go along. A dictation profile other than
        // the active one decides with its own switch whether to post-process
        // (`app_profiles`).
        let end = crate::end_keys::resolve(&profile_values, end, &binding_id);
        log::info!("The recording ends with end shortcut {}", end.id);
        let post_process = crate::app_profiles::effective_post_process(
            end.post_process,
            crate::app_profiles::dictation_values().as_ref(),
        );
        let with_context =
            crate::send_context::decide_at_stop(app, post_process && end.send_context);
        // DanyWhere: the window this dictation ends in, for context from
        // earlier dictations there (`dictation_context`). In the background.
        let context_session = RECORDING_SESSION.load(Ordering::SeqCst);
        crate::window_capture::begin_capture(app, context_session);
        let cancel_generation = rm.cancel_generation();

        tauri::async_runtime::spawn(async move {
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
                    if transcribed_with_end_values {
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

                            if post_process {
                                if use_streaming_overlay {
                                    tm.emit_stream_working(StreamWorkKind::Polishing);
                                } else {
                                    show_processing_overlay(&ah);
                                }
                            }
                            // DanyWhere: the window this dictation started
                            // in, and what the word list replaced in it.
                            let window = crate::window_capture::take_capture(context_session);
                            let context_query =
                                crate::dictation_context::ContextQuery::for_new_dictation(
                                    // Only "Send Context" sends context; every
                                    // dictation keeps its window.
                                    if with_context { window.clone() } else { None },
                                    word_replacements.clone(),
                                );
                            let Some(processed) = complete_unless_cancelled(
                                process_transcription_output(
                                    &ah,
                                    &transcription,
                                    post_process,
                                    &context_query,
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
                                    post_process,
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
                                        crate::profile_redo::remember(
                                            crate::profile_redo::MadeWith {
                                                entry_id: entry.id,
                                                profile_id,
                                                values: profile_values,
                                                end: end.clone(),
                                            },
                                        );
                                        if let Err(err) = hm.record_entry_context(
                                            entry.id,
                                            window.as_ref(),
                                            &word_replacements,
                                            with_context
                                                .then_some(processed.context_entry_ids.as_slice()),
                                        ) {
                                            warn!(
                                                "Failed to store the dictation's window: {}",
                                                err
                                            );
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
                                let ah_clone = ah.clone();
                                let paste_time = Instant::now();
                                let post_process_failed = processed.post_process_failed;
                                let effort_refused_by =
                                    processed.post_process_effort_refused_by.clone();
                                let rm_for_paste = Arc::clone(&rm);
                                let hm_for_paste = Arc::clone(&hm);
                                let tm_for_paste = Arc::clone(&tm);
                                // Taken before the paste thread exists, and so
                                // before the next session can start.
                                let session = RECORDING_SESSION.load(Ordering::SeqCst);
                                // DanyWhere: the dictation profile's values for the
                                // paste and its "Append" field, taken now: the
                                // profile ends when this task returns.
                                let dictation_values = crate::app_profiles::dictation_values();
                                // DanyWhere: and all of them for the paste
                                // thread's sounds and notices.
                                let (_, values_here) =
                                    crate::app_profiles::dictation_profile_and_values(&ah);
                                // Off the main thread: paste sleeps between steps, and a
                                // paste into Handy's own windows needs the main thread free.
                                std::thread::Builder::new()
                                    .name("paste".into())
                                    .spawn(move || {
                                        crate::app_profiles::keep_values_here(values_here);
                                        if rm_for_paste.was_cancelled_since(cancel_generation) {
                                            debug!(
                                                "Transcription operation cancelled before paste"
                                            );
                                            utils::hide_recording_overlay(&ah_clone);
                                            set_tray_state(&ah_clone, TrayIconState::Idle);
                                            return;
                                        }

                                        // A new session may have taken over the
                                        // UI while this paste ran.
                                        let owns_ui =
                                            || RECORDING_SESSION.load(Ordering::SeqCst) == session;
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
                                                &ah_clone,
                                                &hm_for_paste,
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
                                                        show_transcribing_overlay(&ah_clone)
                                                    }
                                                    (false, crate::profile_redo::Working::PostProcessing) => {
                                                        show_processing_overlay(&ah_clone)
                                                    }
                                                },
                                            ) {
                                                Some(insert) => match insert.text {
                                                    Some(text) => (
                                                        tauri::async_runtime::block_on(
                                                            crate::transcription_hook::apply(&ah_clone, text),
                                                        ),
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
                                                    end.append_text.clone(),
                                                    post_process_failed,
                                                    effort_refused_by,
                                                ),
                                            };
                                        // DanyWhere: the "Insert Last Dictation"
                                        // service may be waiting for this text.
                                        let text_for_service = final_text.clone();
                                        // DanyWhere: with its end shortcut's "Append" text.
                                        let service_text = crate::append_text::text_to_paste(
                                            text_for_service,
                                            &append_text,
                                        );
                                        // DanyWhere: "Copy Last Transcript"
                                        // during this dictation puts it on the
                                        // clipboard instead (below).
                                        let pasted = if crate::dictation_key_inserts::copies_instead_of_pasting(session) {
                                            log::info!("Copy Last Transcript asked for this dictation; not pasting it.");
                                            Ok(utils::PasteOutcome::Pasted)
                                        } else {
                                            utils::paste_with_values(final_text, ah_clone.clone(), dictation_values, &append_text)
                                        };
                                        let (outcome, notice) =
                                            match pasted {
                                                Ok(utils::PasteOutcome::Pasted) => {
                                                    debug!(
                                                        "Text pasted successfully in {:?}",
                                                        paste_time.elapsed()
                                                    );
                                                    // Leave a newer session's notice alone.
                                                    if owns_ui() {
                                                        notices::clear(
                                                            &ah_clone,
                                                            NoticeSource::Dictation,
                                                        );
                                                    }
                                                    let notice = pasted_dictation_notice(
                                                        post_process_failed,
                                                        effort_refused_by.as_deref(),
                                                    );
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
                                        let inserted_by_service = outcome
                                            == TranscriptionOutcome::Typed
                                            && crate::insert_service::deliver_to_waiting_call(
                                                &service_text,
                                            );
                                        // DanyWhere: "Copy Last Transcript"
                                        // during this dictation, or an app that
                                        // didn't know the dictation shortcut,
                                        // asked for its text on the clipboard,
                                        // with its "Append" text as the service
                                        // would have inserted it (user, 2026-11-16).
                                        let put_on_clipboard = if outcome
                                            == TranscriptionOutcome::Typed
                                            && !inserted_by_service
                                        {
                                            crate::dictation_key_inserts::put_on_clipboard_if_requested(
                                                &ah_clone,
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
                                            &ah_clone,
                                            crate::delivery::DictationEnd {
                                                outcome,
                                                cancelled: rm_for_paste
                                                    .was_cancelled_since(cancel_generation),
                                                inserted_by_service,
                                                put_on_clipboard: put_on_clipboard.is_some(),
                                                owns_ui: owns_ui(),
                                            },
                                        );
                                        match (owns_ui(), notice, &put_on_clipboard) {
                                            (true, Some(notice), _) => {
                                                notices::raise(&ah_clone, notice)
                                            }
                                            (true, None, _) if inserted_by_service => {
                                                utils::hide_recording_overlay(&ah_clone)
                                            }
                                            (true, None, Some(reason)) => {
                                                crate::dictation_key_inserts::show_on_clipboard_notice(
                                                    &ah_clone,
                                                    reason,
                                                )
                                            }
                                            // DanyWhere: says where the text waits
                                            // when it was not pasted.
                                            (true, None, None) => {
                                                crate::delivery::finish_overlay_after_dictation(
                                                    &ah_clone,
                                                )
                                            }
                                            (false, Some(notice), _) => {
                                                notices::raise_without_overlay(&ah_clone, notice)
                                            }
                                            (false, None, _) => {}
                                        }
                                        if owns_ui() {
                                            set_tray_state(&ah_clone, TrayIconState::Idle);
                                        }
                                        emit_transcription_complete(
                                            &ah_clone,
                                            outcome,
                                            sample_count,
                                            stop_time,
                                        );
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
                                debug!(
                                    "Transcription operation cancelled after transcription error"
                                );
                                utils::hide_recording_overlay(&ah);
                                set_tray_state(&ah, TrayIconState::Idle);
                                return;
                            }

                            error!("Transcription failed: {}", err);
                            notices::raise(
                                &ah,
                                Notice::new(
                                    NoticeCode::TranscriptionFailed,
                                    NoticeSource::Dictation,
                                )
                                .with_detail(err.to_string()),
                            );
                            // Save entry with empty text so user can retry
                            if wav_saved {
                                if let Err(save_err) = hm.save_entry(
                                    file_name,
                                    String::new(),
                                    post_process,
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
        });

        debug!(
            "TranscribeAction::stop completed in {:?}",
            stop_time.elapsed()
        );
    }
}

// Cancel Action
struct CancelAction;

impl ShortcutAction for CancelAction {
    fn start(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        utils::cancel_current_operation(app);
    }

    fn stop(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // Nothing to do on stop for cancel
    }
}

// Paste Last Transcript Action
struct PasteLastTranscriptAction;

impl ShortcutAction for PasteLastTranscriptAction {
    fn start(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // Delivered on release: while the shortcut's modifiers are still held,
        // a simulated Cmd+V would arrive as e.g. Option+Shift+Cmd+V.
    }

    fn stop(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // DanyWhere: without Accessibility it reaches DanyWhere only where it
        // can't be the "Insert Last Dictation" service's shortcut; it copies
        // then.
        crate::dictation_key_inserts::paste_last_transcript(
            app,
            RECORDING_SESSION.load(Ordering::SeqCst),
        );
    }
}

// Copy Last Transcript Action (DanyWhere)
struct CopyLastTranscriptAction;

impl ShortcutAction for CopyLastTranscriptAction {
    fn start(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // Copied on release, like Paste Last Transcript.
    }

    fn stop(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // During a dictation it ends the dictation and puts its text on the
        // clipboard once it's ready.
        crate::dictation_key_inserts::copy_last_transcript(
            app,
            RECORDING_SESSION.load(Ordering::SeqCst),
        );
    }
}

// Switch Word List Action (DanyWhere)
struct CycleWordListAction;

impl ShortcutAction for CycleWordListAction {
    fn start(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // Switched on release, so a held key or key repeat switches once.
    }

    fn stop(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        crate::word_lists::switch_to_next(app);
    }
}

// Choose Profile by Application on/off (DanyWhere, `app_profiles`)
struct ToggleProfileAutoAction;

impl ShortcutAction for ToggleProfileAutoAction {
    fn start(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // Switched on release, so a held key or key repeat switches once.
    }

    fn stop(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // Switching registers and unregisters shortcuts, which the handler
        // thread itself serves: off that thread, or it waits for itself.
        let app = app.clone();
        crate::shortcut::off_handler_thread(move || {
            crate::app_profiles::toggle_from_shortcut(&app);
        });
    }
}

// Test Action
struct TestAction;

impl ShortcutAction for TestAction {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        log::info!(
            "Shortcut ID '{}': Started - {} (App: {})", // Changed "Pressed" to "Started" for consistency
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        log::info!(
            "Shortcut ID '{}': Stopped - {} (App: {})", // Changed "Released" to "Stopped" for consistency
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }
}

// Static Action Map
pub static ACTION_MAP: Lazy<HashMap<String, Arc<dyn ShortcutAction>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "transcribe".to_string(),
        Arc::new(TranscribeAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "transcribe_with_post_process".to_string(),
        Arc::new(TranscribeAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "cancel".to_string(),
        Arc::new(CancelAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "paste_last_transcript".to_string(),
        Arc::new(PasteLastTranscriptAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "copy_last_transcript".to_string(),
        Arc::new(CopyLastTranscriptAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "cycle_word_list".to_string(),
        Arc::new(CycleWordListAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        crate::app_profiles::TOGGLE_BINDING.to_string(),
        Arc::new(ToggleProfileAutoAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "test".to_string(),
        Arc::new(TestAction) as Arc<dyn ShortcutAction>,
    );
    map
});

#[cfg(test)]
mod tests {
    use super::{
        complete_unless_cancelled, is_blank_transcription, non_blank_post_processed_text,
        pasted_dictation_notice, post_process_transcription, post_process_within_time_limit,
        should_use_streaming_overlay, strip_invisible_chars, strip_think_block, PostProcessOutcome,
        PostProcessReport,
    };
    use crate::notices::{Notice, NoticeCode, NoticeSource};
    use crate::settings::{get_default_settings, AppSettings, OverlayStyle};
    use std::future;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    /// Settings that run unstructured post-processing against `base_url`.
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
        // DanyWhere: the first prompt of the library is the active one.
        settings.post_process_prompt_order =
            crate::prompt_library::default_order(&settings.post_process_prompts);
        if let Some(first) = settings.post_process_prompt_order.first_mut() {
            first.active = true;
        }
        settings
    }

    /// Answers one chat completion request with `body`, and returns its URL.
    fn serve_once(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 8192];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        format!("http://{address}")
    }

    /// Accepts one request and never answers it, until the returned sender
    /// is dropped. Returns the URL and that sender.
    fn serve_never() -> (String, std::sync::mpsc::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (hold, release) = std::sync::mpsc::channel::<()>();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 8192];
            let _ = stream.read(&mut request);
            // Keep the connection open without a reply.
            let _ = release.recv();
            drop(stream);
        });
        (format!("http://{address}"), hold)
    }

    #[test]
    fn post_processing_gives_up_at_the_time_limit_from_the_settings() {
        let (url, _hold) = serve_never();
        let mut settings = post_process_settings(&url);
        settings.post_process_timeout_secs = 5;

        let started = std::time::Instant::now();
        let outcome = tauri::async_runtime::block_on(async {
            // Handy's fixed 60 s would run far past this.
            tokio::time::timeout(
                Duration::from_secs(15),
                post_process_within_time_limit(&settings, "hello", None),
            )
            .await
            .map(|report| report.outcome)
        });
        let elapsed = started.elapsed();

        assert_eq!(
            outcome,
            Ok(PostProcessOutcome::Failed),
            "post-processing must give up after the 5 s from the settings"
        );
        assert!(
            elapsed >= Duration::from_millis(4900) && elapsed < Duration::from_secs(10),
            "gave up after {elapsed:?}"
        );
    }

    fn run_post_process(settings: &AppSettings, transcription: &str) -> PostProcessOutcome {
        tauri::async_runtime::block_on(post_process_transcription(settings, transcription, None))
            .outcome
    }

    fn marker_block() -> crate::dictation_context::ContextBlock {
        crate::dictation_context::ContextBlock {
            system: "SYSTEM-MARKER".to_string(),
            legacy: "LEGACY-MARKER".to_string(),
            entry_ids: vec![1],
        }
    }

    /// Like [`serve_once`], and hands over the request it got.
    fn serve_once_capturing(body: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 8192];
            // Read until the body announced by content-length is complete.
            loop {
                let n = stream.read(&mut buffer).unwrap_or(0);
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(header_end) = text.find("\r\n\r\n") {
                    let length = text[..header_end]
                        .lines()
                        .find_map(|line| {
                            let lower = line.to_ascii_lowercase();
                            lower
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&request).to_string());
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        (format!("http://{address}"), rx)
    }

    #[test]
    fn the_word_lists_go_to_post_processing_as_known_terms_before_the_context() {
        let (url, request) = serve_once_capturing(
            r#"{"choices":[{"message":{"content":"{\"transcription\":\"polished\"}"}}]}"#,
        );
        let mut settings = post_process_settings(&url);
        for provider in settings.post_process_providers.iter_mut() {
            provider.supports_structured_output = true;
        }
        settings.custom_words = vec!["BeThLi".to_string()];
        let outcome = tauri::async_runtime::block_on(post_process_transcription(
            &settings,
            "hello",
            Some(&marker_block()),
        ))
        .outcome;
        assert_eq!(outcome, PostProcessOutcome::Done("polished".to_string()));
        let request = request.recv_timeout(Duration::from_secs(5)).unwrap();
        let body = &request[request.find("\r\n\r\n").unwrap() + 4..];
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        let system = json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "system")
            .expect("a system message")["content"]
            .as_str()
            .unwrap()
            .to_string();
        let terms = system.find("<known_terms>").expect("known terms");
        let term = system.find("BeThLi").expect("the term");
        let context = system.find("SYSTEM-MARKER").expect("the context block");
        assert!(terms < term && term < context, "{system}");
    }

    #[test]
    fn the_context_block_follows_the_prompt_in_the_legacy_request() {
        let (url, request) =
            serve_once_capturing(r#"{"choices":[{"message":{"content":"polished"}}]}"#);
        let settings = post_process_settings(&url);
        let outcome = tauri::async_runtime::block_on(post_process_transcription(
            &settings,
            "hello",
            Some(&marker_block()),
        ))
        .outcome;
        assert_eq!(outcome, PostProcessOutcome::Done("polished".to_string()));
        let request = request.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(request.contains("LEGACY-MARKER"), "{request}");
        assert!(!request.contains("SYSTEM-MARKER"), "{request}");
    }

    #[test]
    fn the_context_block_goes_into_the_system_prompt() {
        let (url, request) = serve_once_capturing(
            r#"{"choices":[{"message":{"content":"{\"transcription\":\"polished\"}"}}]}"#,
        );
        let mut settings = post_process_settings(&url);
        for provider in settings.post_process_providers.iter_mut() {
            provider.supports_structured_output = true;
        }
        let outcome = tauri::async_runtime::block_on(post_process_transcription(
            &settings,
            "hello",
            Some(&marker_block()),
        ))
        .outcome;
        assert_eq!(outcome, PostProcessOutcome::Done("polished".to_string()));
        let request = request.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!request.contains("LEGACY-MARKER"), "{request}");
        let body = &request[request.find("\r\n\r\n").unwrap() + 4..];
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        let system = json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "system")
            .expect("a system message");
        assert!(
            system["content"]
                .as_str()
                .unwrap()
                .contains("SYSTEM-MARKER"),
            "{system}"
        );
        let user = json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "user")
            .expect("a user message");
        // DanyWhere: the default prompt is framed, so the transcript comes
        // in tags (`prompt_frame`).
        assert_eq!(user["content"], "<transcript>\nhello\n</transcript>");
    }

    // DanyWhere: the effort (`post_process_effort`), on the request that
    // leaves the computer.

    /// Answers one request per entry of `responses` (status line, body), in
    /// turn, and hands over each request's JSON body.
    fn serve_in_turn(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (String, std::sync::mpsc::Receiver<serde_json::Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
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

    fn settings_with_effort(base_url: &str, effort: &str) -> AppSettings {
        let mut settings = post_process_settings(base_url);
        let provider_id = settings.post_process_provider_id.clone();
        settings
            .post_process_efforts
            .insert(provider_id, effort.to_string());
        settings
    }

    #[test]
    fn the_profiles_effort_goes_with_the_request() {
        let (url, bodies) = serve_in_turn(vec![("200 OK", POLISHED)]);
        let settings = settings_with_effort(&url, "high");
        let report =
            tauri::async_runtime::block_on(post_process_transcription(&settings, "hello", None));
        assert_eq!(
            report,
            PostProcessReport {
                outcome: PostProcessOutcome::Done("polished".to_string()),
                effort_refused_by: None,
            }
        );
        let body = bodies.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(body["reasoning_effort"], "high", "{body}");
    }

    #[test]
    fn without_an_effort_the_request_has_no_reasoning_field() {
        let (url, bodies) = serve_in_turn(vec![("200 OK", POLISHED)]);
        let settings = post_process_settings(&url);
        assert_eq!(
            run_post_process(&settings, "hello"),
            PostProcessOutcome::Done("polished".to_string())
        );
        let body = bodies.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(body.get("reasoning_effort").is_none(), "{body}");
    }

    #[test]
    fn a_refused_effort_still_gives_the_text_and_names_the_model() {
        let (url, bodies) = serve_in_turn(vec![
            (
                "400 Bad Request",
                r#"{"error":"unsupported reasoning_effort"}"#,
            ),
            ("200 OK", POLISHED),
        ]);
        let settings = settings_with_effort(&url, "xhigh");
        let report =
            tauri::async_runtime::block_on(post_process_transcription(&settings, "hello", None));
        assert_eq!(
            report,
            PostProcessReport {
                outcome: PostProcessOutcome::Done("polished".to_string()),
                effort_refused_by: Some("model".to_string()),
            }
        );
        let first = bodies.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(first["reasoning_effort"], "xhigh");
        let retry = bodies.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(retry.get("reasoning_effort").is_none(), "{retry}");
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

    // DanyWhere: the fixed frame (`prompt_frame`), checked on the request
    // that leaves the computer.

    const POLISHED: &str = r#"{"choices":[{"message":{"content":"polished"}}]}"#;
    const POLISHED_STRUCTURED: &str =
        r#"{"choices":[{"message":{"content":"{\"transcription\":\"polished\"}"}}]}"#;

    /// [`post_process_settings`] with `prompt` as the active prompt's text.
    fn settings_with_prompt(base_url: &str, prompt: &str, structured: bool) -> AppSettings {
        let mut settings = post_process_settings(base_url);
        settings.post_process_prompts[0].prompt = prompt.to_string();
        for provider in settings.post_process_providers.iter_mut() {
            provider.supports_structured_output = structured;
        }
        settings
    }

    /// Post-processes `transcription` and returns the request's messages as
    /// (role, content).
    fn sent_messages(
        prompt: &str,
        structured: bool,
        transcription: &str,
        context: Option<&crate::dictation_context::ContextBlock>,
    ) -> Vec<(String, String)> {
        let (url, request) = serve_once_capturing(if structured {
            POLISHED_STRUCTURED
        } else {
            POLISHED
        });
        let settings = settings_with_prompt(&url, prompt, structured);
        let outcome = tauri::async_runtime::block_on(post_process_transcription(
            &settings,
            transcription,
            context,
        ))
        .outcome;
        assert_eq!(outcome, PostProcessOutcome::Done("polished".to_string()));
        let request = request.recv_timeout(Duration::from_secs(5)).unwrap();
        let body = &request[request.find("\r\n\r\n").unwrap() + 4..];
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["role"].as_str().unwrap().to_string(),
                    m["content"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn message(role: &str, content: &str) -> (String, String) {
        (role.to_string(), content.to_string())
    }

    #[test]
    fn a_prompt_without_output_is_framed_in_one_message_the_transcript_last() {
        use crate::prompt_frame::FRAME_TEXT;
        assert_eq!(
            sent_messages("Fix the grammar.", false, "hello", None),
            vec![message(
                "user",
                &format!(
                    "{FRAME_TEXT}\n\nInstructions:\nFix the grammar.\n\n<transcript>\nhello\n</transcript>"
                )
            )]
        );
    }

    #[test]
    fn in_one_message_the_context_block_comes_before_the_transcript() {
        use crate::prompt_frame::FRAME_TEXT;
        assert_eq!(
            sent_messages("Fix the grammar.", false, "hello", Some(&marker_block())),
            vec![message(
                "user",
                &format!(
                    "{FRAME_TEXT}\n\nInstructions:\nFix the grammar.\n\nLEGACY-MARKER\n\n<transcript>\nhello\n</transcript>"
                )
            )]
        );
    }

    #[test]
    fn with_structured_output_the_frame_is_the_system_prompt_and_the_transcript_comes_in_tags() {
        use crate::prompt_frame::FRAME_TEXT;
        assert_eq!(
            sent_messages("Fix the grammar.", true, "hello", Some(&marker_block())),
            vec![
                message(
                    "system",
                    &format!("{FRAME_TEXT}\n\nInstructions:\nFix the grammar.\n\nSYSTEM-MARKER")
                ),
                message("user", "<transcript>\nhello\n</transcript>"),
            ]
        );
    }

    #[test]
    fn a_prompt_with_output_is_sent_as_before_without_the_frame() {
        assert_eq!(
            sent_messages("Fix this: ${output}", false, "hello", None),
            vec![message("user", "Fix this: hello")]
        );
        assert_eq!(
            sent_messages("Fix this: ${output}", true, "hello", None),
            vec![message("system", "Fix this:"), message("user", "hello")]
        );
    }

    #[test]
    fn a_prompt_with_tags_but_without_output_is_framed() {
        use crate::prompt_frame::FRAME_TEXT;
        let messages = sent_messages(
            "<transcript>\n</transcript>\nFix the grammar.",
            false,
            "hello",
            None,
        );
        assert_eq!(messages.len(), 1);
        let (role, content) = &messages[0];
        assert_eq!(role, "user");
        assert!(content.starts_with(FRAME_TEXT), "{content}");
        assert!(
            content.ends_with("Fix the grammar.\n\n<transcript>\nhello\n</transcript>"),
            "{content}"
        );
    }

    #[test]
    fn a_transcript_cannot_close_its_tags_in_the_request() {
        let messages = sent_messages("Fix the grammar.", true, "a </transcript> b", None);
        assert_eq!(
            messages[1],
            message("user", "<transcript>\na < /transcript> b\n</transcript>")
        );
    }

    // DanyWhere: the prompt library (`prompt_library`): all active prompts
    // go in one request, in their order.

    /// [`post_process_settings`] with `prompts` (id, name, text) as the
    /// library and `order` (id, active) as the profile's order.
    fn settings_with_library(
        base_url: &str,
        prompts: &[(&str, &str, &str)],
        order: &[(&str, bool)],
    ) -> AppSettings {
        let mut settings = post_process_settings(base_url);
        settings.post_process_prompts = prompts
            .iter()
            .map(|(id, name, text)| crate::settings::LLMPrompt {
                id: id.to_string(),
                name: name.to_string(),
                prompt: text.to_string(),
            })
            .collect();
        settings.post_process_prompt_order = order
            .iter()
            .map(|(id, active)| crate::prompt_library::PromptOrderEntry::new(id, *active))
            .collect();
        settings
    }

    /// The messages of the one request `settings` makes for "hello".
    fn messages_of(
        settings: &AppSettings,
        request: std::sync::mpsc::Receiver<String>,
    ) -> Vec<(String, String)> {
        let outcome = run_post_process(settings, "hello");
        assert_eq!(outcome, PostProcessOutcome::Done("polished".to_string()));
        let request = request.recv_timeout(Duration::from_secs(5)).unwrap();
        let body = &request[request.find("\r\n\r\n").unwrap() + 4..];
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["role"].as_str().unwrap().to_string(),
                    m["content"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    const LIBRARY: [(&str, &str, &str); 3] = [
        ("a", "Diktat", "Clean it up."),
        ("b", "Absätze", "Add paragraphs."),
        ("c", "Mail", "Make an email of it."),
    ];

    #[test]
    fn the_active_prompts_go_in_one_request_in_the_profiles_order() {
        use crate::prompt_frame::FRAME_TEXT;
        let (url, request) = serve_once_capturing(POLISHED);
        let settings =
            settings_with_library(&url, &LIBRARY, &[("b", true), ("c", false), ("a", true)]);
        assert_eq!(
            messages_of(&settings, request),
            vec![message(
                "user",
                &format!(
                    "{FRAME_TEXT}\n\nInstructions:\n<instructions name=\"Absätze\">\nAdd paragraphs.\n</instructions>\n\n<instructions name=\"Diktat\">\nClean it up.\n</instructions>\n\n<transcript>\nhello\n</transcript>"
                )
            )]
        );
    }

    #[test]
    fn one_active_prompt_is_sent_as_before() {
        use crate::prompt_frame::FRAME_TEXT;
        let (url, request) = serve_once_capturing(POLISHED);
        let settings =
            settings_with_library(&url, &LIBRARY, &[("a", false), ("c", true), ("b", false)]);
        assert_eq!(
            messages_of(&settings, request),
            vec![message(
                "user",
                &format!(
                    "{FRAME_TEXT}\n\nInstructions:\nMake an email of it.\n\n<transcript>\nhello\n</transcript>"
                )
            )]
        );
    }

    #[test]
    fn an_active_prompt_with_output_runs_alone_as_before() {
        let (url, request) = serve_once_capturing(POLISHED);
        let mut library = LIBRARY.to_vec();
        library.push(("old", "Old", "Fix this: ${output}"));
        let settings =
            settings_with_library(&url, &library, &[("a", true), ("old", true), ("b", true)]);
        assert_eq!(
            messages_of(&settings, request),
            vec![message("user", "Fix this: hello")]
        );
    }

    #[test]
    fn without_an_active_prompt_post_processing_fails() {
        let mut settings = settings_with_library(
            "http://127.0.0.1:1",
            &LIBRARY,
            &[("a", false), ("b", false), ("c", false)],
        );
        // Handy's selected prompt no longer counts.
        settings.post_process_selected_prompt_id = Some("a".to_string());
        assert_eq!(
            run_post_process(&settings, "hello"),
            PostProcessOutcome::Failed
        );
    }

    #[test]
    fn blank_instructions_are_not_post_processed() {
        let settings = settings_with_prompt("http://127.0.0.1:1", " \n ", false);
        assert_eq!(
            run_post_process(&settings, "hello"),
            PostProcessOutcome::Failed
        );
    }

    #[test]
    fn blank_speech_is_skipped_but_missing_setup_failed() {
        let settings = post_process_settings("http://127.0.0.1:1");
        assert_eq!(
            run_post_process(&settings, "  "),
            PostProcessOutcome::Skipped
        );

        let mut settings = post_process_settings("http://127.0.0.1:1");
        settings.post_process_models.clear();
        assert_eq!(
            run_post_process(&settings, "hello"),
            PostProcessOutcome::Failed
        );
    }

    #[test]
    fn a_post_processing_reply_is_done() {
        let url = serve_once(r#"{"choices":[{"message":{"content":"polished"}}]}"#);
        assert_eq!(
            run_post_process(&post_process_settings(&url), "hello"),
            PostProcessOutcome::Done("polished".to_string())
        );
    }

    #[test]
    fn a_post_processing_reply_with_no_content_failed() {
        let url = serve_once(r#"{"choices":[]}"#);
        assert_eq!(
            run_post_process(&post_process_settings(&url), "hello"),
            PostProcessOutcome::Failed
        );
    }

    #[test]
    fn every_default_shortcut_has_an_action() {
        for id in crate::settings::get_default_settings().bindings.keys() {
            assert!(
                super::ACTION_MAP.contains_key(id),
                "no action for default shortcut '{id}'"
            );
        }
    }

    #[test]
    fn blank_transcription_is_detected() {
        assert!(is_blank_transcription(""));
        assert!(is_blank_transcription("   "));
        assert!(is_blank_transcription("\t\n  \r\n"));
    }

    #[test]
    fn non_blank_transcription_is_kept() {
        assert!(!is_blank_transcription("hello"));
        assert!(!is_blank_transcription("  hello  "));
    }

    #[test]
    fn blank_post_processed_text_is_rejected() {
        for text in ["", "   ", "\t\n\r\n", "\u{00A0}\u{2003}"] {
            assert_eq!(non_blank_post_processed_text(text.to_string()), None);
        }
    }

    #[test]
    fn cleaned_empty_post_processed_text_is_rejected() {
        for text in [
            "\u{200B}\u{200C}\u{200D}\u{FEFF}",
            "<think>reasoning only</think>",
            "<think>reasoning only</think>\n\u{200B} ",
        ] {
            let text = strip_invisible_chars(strip_think_block(text));
            assert_eq!(non_blank_post_processed_text(text), None);
        }
    }

    #[test]
    fn non_blank_post_processed_text_is_preserved() {
        for text in ["Cleaned text.", "  정리된 문장입니다.\n", "0"] {
            assert_eq!(
                non_blank_post_processed_text(text.to_string()),
                Some(text.to_string())
            );
        }
    }

    #[test]
    fn completed_operation_returns_its_output() {
        let result = tauri::async_runtime::block_on(complete_unless_cancelled(
            future::ready("done"),
            || false,
        ));

        assert_eq!(result, Some("done"));
    }

    #[test]
    fn pending_operation_stops_after_cancellation() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_for_thread = Arc::clone(&cancelled);
        let cancel_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            cancelled_for_thread.store(true, Ordering::Release);
        });

        let result = tauri::async_runtime::block_on(complete_unless_cancelled(
            future::pending::<()>(),
            || cancelled.load(Ordering::Acquire),
        ));

        cancel_thread.join().unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn leading_think_block_is_stripped() {
        assert_eq!(
            strip_think_block("<think>pondering...</think>Cleaned text."),
            "Cleaned text."
        );
        assert_eq!(
            strip_think_block("  \n<think>multi\nline</think>\n  Cleaned text."),
            "Cleaned text."
        );
    }

    #[test]
    fn content_without_think_block_is_unchanged() {
        assert_eq!(strip_think_block("Cleaned text."), "Cleaned text.");
        assert_eq!(
            strip_think_block("Mentions <think> mid-sentence."),
            "Mentions <think> mid-sentence."
        );
        // Unclosed block: leave untouched rather than guess
        assert_eq!(
            strip_think_block("<think>never closed"),
            "<think>never closed"
        );
    }

    #[test]
    fn live_overlay_uses_streaming_states_only_for_streaming_models() {
        assert!(should_use_streaming_overlay(OverlayStyle::Live, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::Live, false));
        assert!(!should_use_streaming_overlay(OverlayStyle::Minimal, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::None, true));
    }
}
