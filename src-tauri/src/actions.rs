#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::audio_feedback::{play_feedback_sound, play_feedback_sound_blocking, SoundType};
use crate::audio_toolkit::VadPolicy;
use crate::managers::audio::{recording_notice_code, AudioRecordingManager};
use crate::managers::history::HistoryManager;
use crate::managers::model::ModelManager;
use crate::managers::transcription::policy::{live_decode, LiveDecode, ModelTraits};
use crate::managers::transcription::StreamWorkKind;
use crate::managers::transcription::{Transcription, TranscriptionManager};
use crate::notices::{self, Notice, NoticeCode, NoticeSource};
use crate::settings::{get_settings, AppSettings, OverlayStyle, APPLE_INTELLIGENCE_PROVIDER_ID};
use crate::shortcut;
use crate::tray::{set_tray_state, TrayIconState};
use crate::utils::{
    self, show_processing_overlay, show_recording_overlay, show_transcribing_overlay,
};
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

// Transcribe Action
struct TranscribeAction {
    post_process: bool,
    /// DanyWhere: post-process with context from earlier dictations in the
    /// same window (`dictation_context`); only with `post_process`.
    with_context: bool,
}

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

/// DanyWhere: [`post_process_transcription`] under the time limit from the
/// settings (Handy: a fixed 60 s). On timeout it falls back like any other
/// failure, so the original transcription goes out.
async fn post_process_within_time_limit(
    settings: &AppSettings,
    transcription: &str,
    context: Option<&crate::dictation_context::ContextBlock>,
) -> PostProcessOutcome {
    let limit = crate::llm_timeouts::post_process_timeout(settings);
    match tokio::time::timeout(
        limit,
        post_process_transcription(settings, transcription, context),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => {
            warn!(
                "Post-processing timed out after {:?}; pasting the original transcription",
                limit
            );
            PostProcessOutcome::Failed
        }
    }
}

/// DanyWhere: `context` (earlier dictations from the same window, see
/// `dictation_context`) goes after the selected prompt, in every path.
async fn post_process_transcription(
    settings: &AppSettings,
    transcription: &str,
    context: Option<&crate::dictation_context::ContextBlock>,
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

    let selected_prompt_id = match &settings.post_process_selected_prompt_id {
        Some(id) => id.clone(),
        None => {
            debug!("Post-processing skipped because no prompt is selected");
            return PostProcessOutcome::Failed;
        }
    };

    let prompt = match settings
        .post_process_prompts
        .iter()
        .find(|prompt| prompt.id == selected_prompt_id)
    {
        Some(prompt) => prompt.prompt.clone(),
        None => {
            debug!(
                "Post-processing skipped because prompt '{}' was not found",
                selected_prompt_id
            );
            return PostProcessOutcome::Failed;
        }
    };

    if prompt.trim().is_empty() {
        debug!("Post-processing skipped because the selected prompt is empty");
        return PostProcessOutcome::Failed;
    }

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
    let disable_reasoning = matches!(provider.id.as_str(), "custom" | "openrouter");

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
            disable_reasoning,
        )
        .await
        {
            Ok(Some(content)) => {
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
            Ok(None) => {
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
        disable_reasoning,
    )
    .await
    {
        Ok(Some(content)) => {
            let content = strip_invisible_chars(strip_think_block(&content));
            debug!(
                "LLM post-processing succeeded for provider '{}'. Output length: {} chars",
                provider.id,
                content.len()
            );
            PostProcessOutcome::Done(content)
        }
        Ok(None) => {
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
    /// `final_text` is the bare transcript because post-processing failed.
    pub post_process_failed: bool,
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
    // DanyWhere: with the values of the dictation's own profile, if any.
    let settings = crate::app_profiles::dictation_settings(app);
    let mut final_text = transcription.to_string();
    let mut post_processed_text: Option<String> = None;
    let mut post_process_prompt: Option<String> = None;
    let mut post_process_failed = false;
    let mut context_entry_ids: Vec<i64> = Vec::new();

    if post_process {
        // DanyWhere: earlier dictations from the same window, if the context
        // shortcut asked for them (the query then has a window).
        let selected_prompt_id = settings.post_process_selected_prompt_id.as_deref();
        let selected_prompt = settings
            .post_process_prompts
            .iter()
            .find(|prompt| Some(prompt.id.as_str()) == selected_prompt_id)
            .map(|prompt| prompt.prompt.clone())
            .unwrap_or_default();
        let context_block = if is_blank_transcription(&final_text) {
            None
        } else {
            crate::dictation_context::context_for(app, &settings, &selected_prompt, context)
        };
        // A reply that's blank once cleaned is no better than none.
        let outcome =
            match post_process_within_time_limit(&settings, &final_text, context_block.as_ref())
                .await
            {
                PostProcessOutcome::Done(text) => non_blank_post_processed_text(text)
                    .map_or(PostProcessOutcome::Failed, PostProcessOutcome::Done),
                outcome => outcome,
            };
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

                if let Some(prompt_id) = &settings.post_process_selected_prompt_id {
                    if let Some(prompt) = settings
                        .post_process_prompts
                        .iter()
                        .find(|prompt| &prompt.id == prompt_id)
                    {
                        post_process_prompt = Some(prompt.prompt.clone());
                    }
                }
            }
        }
    }

    ProcessedTranscription {
        final_text,
        post_processed_text,
        post_process_prompt,
        post_process_failed,
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
        let session = RECORDING_SESSION.fetch_add(1, Ordering::SeqCst) + 1;
        // DanyWhere: which window this dictation starts in, for context from
        // earlier dictations there (`dictation_context`). In the background.
        crate::window_capture::begin_capture(app, session);
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

    fn stop(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
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

        set_tray_state(app, TrayIconState::Transcribing);
        // Stop should give immediate visual feedback. Live streaming can keep
        // the larger panel, but it still switches from listening to a working
        // spinner while the stream finalizes. Non-streaming paths use the
        // compact transcribing pill (None no-ops in show_*).
        let style = get_settings(app).overlay_style;
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
                                                 // DanyWhere: a dictation profile other than the active one decides
                                                 // with its own switch whether to post-process (`app_profiles`).
        let post_process = crate::app_profiles::effective_post_process(
            self.post_process,
            crate::app_profiles::dictation_values().as_ref(),
        );
        let with_context = post_process && self.with_context;
        // DanyWhere: the recording whose window was captured at its start.
        let context_session = RECORDING_SESSION.load(Ordering::SeqCst);
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
                                    // Only the context shortcut sends context;
                                    // every dictation keeps its window.
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
                            if wav_saved {
                                match hm.save_entry(
                                    file_name,
                                    transcription,
                                    post_process,
                                    processed.post_processed_text.clone(),
                                    processed.post_process_prompt.clone(),
                                    language,
                                ) {
                                    // DanyWhere: its window and context.
                                    Ok(entry) => {
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
                                let rm_for_paste = Arc::clone(&rm);
                                // Taken before the paste thread exists, and so
                                // before the next session can start.
                                let session = RECORDING_SESSION.load(Ordering::SeqCst);
                                // Off the main thread: paste sleeps between steps, and a
                                // paste into Handy's own windows needs the main thread free.
                                std::thread::Builder::new()
                                    .name("paste".into())
                                    .spawn(move || {
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
                                        // DanyWhere: the "Insert Last Dictation"
                                        // service may be waiting for this text.
                                        let text_for_service = final_text.clone();
                                        // DanyWhere: "Copy Last Transcript"
                                        // during this dictation puts it on the
                                        // clipboard instead (below).
                                        let pasted = if crate::dictation_key_inserts::copies_instead_of_pasting(session) {
                                            log::info!("Copy Last Transcript asked for this dictation; not pasting it.");
                                            Ok(utils::PasteOutcome::Pasted)
                                        } else {
                                            utils::paste(final_text, ah_clone.clone())
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
                                                    let notice = post_process_failed.then(|| {
                                                        Notice::new(
                                                            NoticeCode::PostProcessFailed,
                                                            NoticeSource::Dictation,
                                                        )
                                                    });
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
                                                &text_for_service,
                                            );
                                        // DanyWhere: "Copy Last Transcript"
                                        // during this dictation, or an app that
                                        // didn't know the dictation shortcut,
                                        // asked for its text on the clipboard.
                                        let put_on_clipboard = if outcome
                                            == TranscriptionOutcome::Typed
                                            && !inserted_by_service
                                        {
                                            crate::dictation_key_inserts::put_on_clipboard_if_requested(
                                                &ah_clone,
                                                session,
                                                &text_for_service,
                                            )
                                        } else {
                                            None
                                        };
                                        // DanyWhere: a sound says the text waits
                                        // to be pasted or inserted, also when post-
                                        // processing failed; not while a newer
                                        // session records.
                                        if outcome == TranscriptionOutcome::Typed
                                            && owns_ui()
                                            && !inserted_by_service
                                        {
                                            crate::delivery::play_sound_if_waiting_for_user(
                                                &ah_clone,
                                                put_on_clipboard.is_some(),
                                            );
                                        }
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
                                    None,
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
        Arc::new(TranscribeAction {
            post_process: false,
            with_context: false,
        }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "transcribe_with_post_process".to_string(),
        Arc::new(TranscribeAction {
            post_process: true,
            with_context: false,
        }) as Arc<dyn ShortcutAction>,
    );
    // DanyWhere: dictate, then post-process with context from earlier
    // dictations in the same window (`dictation_context`).
    map.insert(
        crate::dictation_context::CONTEXT_BINDING.to_string(),
        Arc::new(TranscribeAction {
            post_process: true,
            with_context: true,
        }) as Arc<dyn ShortcutAction>,
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
        post_process_transcription, post_process_within_time_limit, should_use_streaming_overlay,
        strip_invisible_chars, strip_think_block, PostProcessOutcome,
    };
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
        settings.post_process_selected_prompt_id =
            settings.post_process_prompts.first().map(|p| p.id.clone());
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
    fn the_context_block_follows_the_prompt_in_the_legacy_request() {
        let (url, request) =
            serve_once_capturing(r#"{"choices":[{"message":{"content":"polished"}}]}"#);
        let settings = post_process_settings(&url);
        let outcome = tauri::async_runtime::block_on(post_process_transcription(
            &settings,
            "hello",
            Some(&marker_block()),
        ));
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
        ));
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

    // DanyWhere: the fixed frame (`prompt_frame`), checked on the request
    // that leaves the computer.

    const POLISHED: &str = r#"{"choices":[{"message":{"content":"polished"}}]}"#;
    const POLISHED_STRUCTURED: &str =
        r#"{"choices":[{"message":{"content":"{\"transcription\":\"polished\"}"}}]}"#;

    /// [`post_process_settings`] with `prompt` as the selected prompt's text.
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
        ));
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
