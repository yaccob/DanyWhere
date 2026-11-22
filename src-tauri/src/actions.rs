#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::audio_feedback::{play_feedback_sound, play_feedback_sound_blocking, SoundType};
use crate::audio_toolkit::VadPolicy;
use crate::managers::audio::{recording_notice_code, AudioRecordingManager};
use crate::managers::model::ModelManager;
use crate::managers::transcription::policy::{live_decode, LiveDecode, ModelTraits};
use crate::managers::transcription::StreamWorkKind;
use crate::managers::transcription::TranscriptionManager;
use crate::notices::{self, Notice, NoticeSource};
use crate::settings::{AppSettings, OverlayStyle, APPLE_INTELLIGENCE_PROVIDER_ID};
use crate::shortcut;
use crate::tray::{set_tray_state, TrayIconState};
use crate::utils::{self, show_recording_overlay, show_transcribing_overlay};
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

pub(crate) fn emit_transcription_complete(
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

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Bumped per recording session, so a late paste from an earlier session
/// doesn't reset the tray or hide the overlay of a newer one.
static RECORDING_SESSION: AtomicU64 = AtomicU64::new(0);

/// DanyWhere: the current recording session, for `dictation_key_inserts`.
pub fn current_recording_session() -> u64 {
    RECORDING_SESSION.load(Ordering::SeqCst)
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

pub(crate) async fn complete_unless_cancelled<F, C>(
    operation: F,
    is_cancelled: C,
) -> Option<F::Output>
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
    process_transcription_with_settings(&settings, transcription, post_process, |instructions| {
        crate::dictation_context::context_for(app, &settings, instructions, context)
    })
    .await
}

/// DanyWhere: [`process_transcription_output_with`] with the `settings`
/// given (a dictation's, `dictation_snapshot`) instead of read through the
/// app. `context_for` builds the context block for the instructions it is
/// given (`dictation_context::context_for`); it is asked only when the
/// transcription is post-processed and has speech.
pub(crate) async fn process_transcription_with_settings(
    settings: &AppSettings,
    transcription: &str,
    post_process: bool,
    context_for: impl FnOnce(&str) -> Option<crate::dictation_context::ContextBlock>,
) -> ProcessedTranscription {
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
            let prompts = crate::prompt_library::requested(settings);
            (
                crate::prompt_library::request_instructions(&prompts),
                crate::prompt_library::request_names(&prompts),
            )
        };
        let context_block = if is_blank_transcription(&final_text) {
            None
        } else {
            context_for(&instructions)
        };
        // A reply that's blank once cleaned is no better than none.
        let report =
            post_process_within_time_limit(settings, &final_text, context_block.as_ref()).await;
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

impl ShortcutAction for TranscribeAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let start_time = Instant::now();
        debug!("TranscribeAction::start called for binding: {}", binding_id);

        // Load model in the background
        let tm = app.state::<Arc<TranscriptionManager>>();
        let rm = app.state::<Arc<AudioRecordingManager>>();

        // DanyWhere: the profile of the app in front, when it has one of
        // its own (`app_profiles`); its values apply until the dictation ends.
        // What the overlay shows of the dictation's profile comes back.
        let dictation_profile = crate::app_profiles::begin_dictation(app);

        // Load ASR model and VAD model in parallel
        let kickoff_started = Instant::now();
        // An empty selection has nothing to load; the check below says so.
        let selected_model = crate::app_profiles::dictation_settings(app).selected_model;
        let load_started = !selected_model.is_empty() && tm.initiate_model_load(true);
        let rm_clone = Arc::clone(&rm);
        std::thread::spawn(move || {
            if let Err(e) = rm_clone.preload_vad() {
                debug!("VAD pre-load failed: {}", e);
            }
        });
        let kickoff_elapsed = kickoff_started.elapsed();

        // Don't open the mic if nothing can transcribe the recording. A load
        // started above reports its own failure; one already running doesn't.
        // DanyWhere: also when another model than this dictation's is loaded.
        if !tm.is_model_loaded() || tm.get_current_model().as_deref() != Some(&selected_model) {
            if selected_model.is_empty() {
                warn!("Not starting recording: no model is selected");
                crate::app_profiles::end_dictation();
                tm.raise_no_model_selected();
                return;
            }
            if let Err(e) = app
                .state::<Arc<ModelManager>>()
                .get_model_path(&selected_model)
            {
                warn!("Not starting recording: no model can transcribe it ({})", e);
                crate::app_profiles::end_dictation();
                if !load_started {
                    tm.raise_model_load_failed(&selected_model, &e);
                }
                return;
            }
        }

        let binding_id = binding_id.to_string();
        // DanyWhere: the dictation's window is captured when its recording
        // ends (`stop`), as its profile is chosen then.
        RECORDING_SESSION.fetch_add(1, Ordering::SeqCst);
        let tray_started = Instant::now();
        set_tray_state(app, TrayIconState::Recording);
        let tray_elapsed = tray_started.elapsed();

        // Get the microphone mode to determine audio feedback timing
        let plan_started = Instant::now();
        // DanyWhere: with the values of the dictation's own profile, if any.
        let settings = crate::app_profiles::dictation_settings(app);
        let is_always_on = settings.always_on_microphone;

        let selected_model_info = app
            .state::<Arc<ModelManager>>()
            .get_model_info(&settings.selected_model);

        // Use the app-facing model capability as the single pre-recording source
        // for live streaming decisions. Unknown support is represented as false
        // until the model registry is updated by discovery or runtime load.
        let model_supports_streaming = selected_model_info
            .as_ref()
            .map(|m| m.supports_streaming)
            .unwrap_or(false);
        let vad_policy = if !settings.vad_enabled {
            VadPolicy::Disabled
        } else if model_supports_streaming {
            VadPolicy::Streaming
        } else {
            VadPolicy::Offline
        };
        // Before the model loads, whether it keeps its accuracy on long audio
        // is unknown, so assume not: the worker decides again with the loaded
        // model, and a wrong guess costs only an idle worker.
        let expected_model = ModelTraits {
            streaming: model_supports_streaming,
            long_form: false,
        };
        if live_decode(expected_model, settings.long_form_chunking) != LiveDecode::None {
            tm.start_stream();
        }
        let plan_elapsed = plan_started.elapsed();

        // Sizing the overlay follows the same advertised capability. A model that
        // doesn't stream (or whose capability is not known yet) gets the compact
        // pill instead of an oversized transparent live window.
        let overlay_started = Instant::now();
        // DanyWhere: from two profiles on, the overlay names the dictation's
        // profile with the glyph of choosing it by application.
        crate::overlay::set_dictation_profile(app, dictation_profile);
        // DanyWhere: and a speech bubble while the end it is expected to
        // take (`end_keys::expected_end`) sends context.
        let expected_end = crate::end_keys::expected_end(
            &crate::app_profiles::dictation_profile_and_values(app).1,
            &binding_id,
            settings.shortcut_activation,
        );
        let expected_post_process = crate::app_profiles::effective_post_process(
            expected_end.post_process,
            crate::app_profiles::dictation_values().as_ref(),
        );
        crate::send_context::begin_dictation(
            app,
            expected_post_process && expected_end.send_context,
        );
        match settings.overlay_style {
            OverlayStyle::Live if model_supports_streaming => utils::show_streaming_overlay(app),
            OverlayStyle::Live | OverlayStyle::Minimal => show_recording_overlay(app),
            OverlayStyle::None => {} // show_overlay_state no-ops on None anyway
        }
        // Everything above runs before capture can begin, so each span here is
        // added keypress->capture latency.
        debug!(
            "start-path pre-recording steps: model_kickoff={:?} tray={:?} settings+stream_plan={:?} overlay={:?}",
            kickoff_elapsed,
            tray_elapsed,
            plan_elapsed,
            overlay_started.elapsed()
        );
        debug!("Microphone mode - always_on: {}", is_always_on);

        let mut recording_error: Option<anyhow::Error> = None;
        let recording_start_time = Instant::now();
        match rm.try_start_recording(&binding_id, vad_policy) {
            Ok(readiness) => {
                debug!(
                    "Recording request accepted in {:?}; waiting for first microphone samples",
                    recording_start_time.elapsed()
                );
                // DanyWhere: an app that answers its first Apple event slowly
                // is asked now, so the window title comes in time when the
                // recording ends (`window_capture`, macOS without Accessibility).
                crate::window_capture::prime_title_query(app);
                let generation = readiness.generation();
                let app_clone = app.clone();
                let rm_clone = Arc::clone(&rm);
                std::thread::spawn(move || {
                    if !readiness.wait() {
                        debug!("Microphone readiness wait ended without receiving samples");
                        return;
                    }

                    // Development-only preview hook for evaluating the brief
                    // arming animation on hardware that normally starts too fast
                    // to make it visible.
                    #[cfg(debug_assertions)]
                    if let Ok(delay_ms) = std::env::var("HANDY_DEBUG_MIC_READY_DELAY_MS")
                        .unwrap_or_default()
                        .parse::<u64>()
                    {
                        let delay_ms = delay_ms.min(10_000);
                        if delay_ms > 0 {
                            debug!("Delaying microphone-ready cue by {delay_ms}ms for UI preview");
                            std::thread::sleep(Duration::from_millis(delay_ms));
                        }
                    }

                    if !rm_clone.is_recording_readiness_current(generation) {
                        debug!("Microphone became ready for an inactive recording");
                        return;
                    }

                    debug!("Microphone is receiving samples; recording is ready");
                    utils::emit_recording_ready(&app_clone);

                    // The start chime is a readiness cue, so it must follow the
                    // first real input callback rather than Stream::play() or a
                    // fixed delay. The helper returns immediately when feedback
                    // is disabled; mute still follows the same readiness point.
                    if rm_clone.is_recording_readiness_current(generation) {
                        play_feedback_sound_blocking(&app_clone, SoundType::Start);
                    }
                    if rm_clone.is_recording_readiness_current(generation) {
                        rm_clone.apply_mute();
                    }
                });
            }
            Err(e) => {
                debug!("Failed to start recording: {}", e);
                recording_error = Some(e);
            }
        }

        if recording_error.is_none() {
            // Dynamically register the cancel shortcut in a separate task to avoid deadlock
            shortcut::register_cancel_shortcut(app);
            // DanyWhere: a post-processing model on this computer starts
            // loading now, so its cold start falls into the speaking time.
            crate::model_warmup::start(&settings, expected_post_process);
        } else {
            // Starting failed (for example due to blocked microphone permissions).
            // Revert UI state so we don't stay stuck in the recording overlay.
            tm.cancel_stream();
            crate::app_profiles::end_dictation();
            set_tray_state(app, TrayIconState::Idle);
            if let Some(err) = recording_error {
                notices::raise(
                    app,
                    Notice::new(recording_notice_code(&err), NoticeSource::Dictation)
                        .with_detail(format!("{err:#}")),
                );
            }
        }

        debug!(
            "TranscribeAction::start completed in {:?}",
            start_time.elapsed()
        );
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

        let rm = Arc::clone(&app.state::<Arc<AudioRecordingManager>>());
        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());

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
        // DanyWhere: everything the rest of this dictation runs with, read
        // once now that its profile is settled (`dictation_snapshot`): its
        // profile's settings and values, the end it ends with, whether it
        // post-processes and sends context, its session and cancel
        // generation.
        let snapshot =
            crate::dictation_snapshot::take(app, end, binding_id, transcribed_with_end_values);

        set_tray_state(app, TrayIconState::Transcribing);
        // Stop should give immediate visual feedback. Live streaming can keep
        // the larger panel, but it still switches from listening to a working
        // spinner while the stream finalizes. Non-streaming paths use the
        // compact transcribing pill (None no-ops in show_*).
        // DanyWhere: the dictation's profile's style; the snapshot read
        // whether a stream runs before it is finalized, so every later
        // working state targets the same overlay that was shown for this
        // transcription.
        let use_streaming_overlay = snapshot.use_streaming_overlay;
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

        // DanyWhere: the end shortcut it ended with, in the profile it ends
        // with (`end_keys`, resolved in the snapshot).
        log::info!("The recording ends with end shortcut {}", snapshot.end.id);
        crate::send_context::decide_at_stop(app, snapshot.with_context);
        // DanyWhere: the window this dictation ends in, for context from
        // earlier dictations there (`dictation_context`). In the background.
        crate::window_capture::begin_capture(&snapshot.settings, snapshot.session);

        tauri::async_runtime::spawn(crate::dictation_flow::finish_dictation(
            app.clone(),
            snapshot,
            binding_id,
            stop_time,
        ));

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
        post_process_transcription, post_process_within_time_limit, strip_invisible_chars,
        strip_think_block, PostProcessOutcome, PostProcessReport,
    };
    use crate::settings::{get_default_settings, AppSettings};
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

    /// `process_transcription_with_settings` with `settings`; the context is
    /// answered with `block`. Also returns the instructions it was asked
    /// for.
    fn process_with(
        settings: &AppSettings,
        transcription: &str,
        post_process: bool,
        block: Option<crate::dictation_context::ContextBlock>,
    ) -> (super::ProcessedTranscription, Vec<String>) {
        let asked = std::sync::Mutex::new(Vec::new());
        let processed = tauri::async_runtime::block_on(super::process_transcription_with_settings(
            settings,
            transcription,
            post_process,
            |instructions| {
                asked.lock().unwrap().push(instructions.to_string());
                block
            },
        ));
        (processed, asked.into_inner().unwrap())
    }

    /// Never contacted: nothing listens there.
    const NOWHERE: &str = "http://127.0.0.1:9";

    #[test]
    fn without_post_processing_the_transcript_goes_out_as_it_is() {
        let settings = post_process_settings(NOWHERE);
        let (processed, asked) = process_with(&settings, "hello", false, Some(marker_block()));
        assert_eq!(processed.final_text, "hello");
        assert_eq!(processed.post_processed_text, None);
        assert!(!processed.post_process_failed);
        assert!(processed.context_entry_ids.is_empty());
        assert!(asked.is_empty(), "no context without post-processing");
    }

    #[test]
    fn post_processing_runs_with_the_settings_it_is_given() {
        let url = serve_once(r#"{"choices":[{"message":{"content":"polished"}}]}"#);
        let settings = post_process_settings(&url);
        let (processed, asked) = process_with(&settings, "hello", true, Some(marker_block()));
        assert_eq!(processed.final_text, "polished");
        assert_eq!(processed.post_processed_text.as_deref(), Some("polished"));
        assert!(!processed.post_process_failed);
        let instructions = crate::prompt_library::request_instructions(
            &crate::prompt_library::requested(&settings),
        );
        assert_eq!(
            processed.post_process_prompt.as_deref(),
            Some(instructions.as_str())
        );
        // The context was built for these instructions, and went out with
        // the request.
        assert_eq!(asked, vec![instructions]);
        assert_eq!(processed.context_entry_ids, vec![1]);
    }

    #[test]
    fn blank_speech_is_not_post_processed_and_asks_for_no_context() {
        let settings = post_process_settings(NOWHERE);
        let (processed, asked) = process_with(&settings, "  ", true, Some(marker_block()));
        assert_eq!(processed.final_text, "  ");
        assert!(!processed.post_process_failed);
        assert!(processed.context_entry_ids.is_empty());
        assert!(asked.is_empty());
    }

    #[test]
    fn a_failed_post_processing_keeps_the_transcript_but_its_context_went_out() {
        let url = serve_once(r#"{"choices":[]}"#);
        let (processed, asked) = process_with(
            &post_process_settings(&url),
            "hello",
            true,
            Some(marker_block()),
        );
        assert_eq!(processed.final_text, "hello");
        assert_eq!(processed.post_processed_text, None);
        assert!(processed.post_process_failed);
        assert_eq!(asked.len(), 1);
        assert_eq!(processed.context_entry_ids, vec![1]);
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
}
