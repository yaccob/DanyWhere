//! DanyWhere: the word list check (word lists stage 2).
//!
//! Only speech models that take a vocabulary (Whisper) hear the word lists.
//! A model without (Canary, Parakeet …) writes a name or term from the lists
//! as similar-sounding words. The fuzzy match finds such words
//! (`audio_toolkit::word_list_candidates`), but it also finds right words
//! that merely sound like a term ("Bild" for "Build"), so it replaces
//! nothing by itself: what it finds are candidates.
//!
//! When the user switched the check on and the dictation isn't
//! post-processed, a small request to the post-processing provider decides
//! from the sentence, per candidate, whether the speaker meant the term;
//! only confirmed candidates are replaced (`dictation_flow`). A
//! post-processed dictation gets its candidates as a hint in the
//! `<known_terms>` block instead (`prompt_frame`), switch or not.

use std::sync::atomic::{AtomicBool, Ordering};

use log::{debug, info, warn};

use crate::audio_toolkit::{word_list_candidates, WordListMatch};
use crate::llm_client::{Completion, ReasoningRequest};
use crate::settings::{AppSettings, PostProcessProvider, APPLE_INTELLIGENCE_PROVIDER_ID};

/// The fuzzy match's threshold for a candidate. Chosen by a measurement
/// over the user's History (sidecar note
/// `qa/2026-11-24_0130_wortlisten-llm-pruefung.md`): at 0.22, 37 of 1193
/// dictations (3.1 %) had a candidate, and it finds real mishearings that
/// 0.18 misses; above 0.25 only junk is added.
pub const CANDIDATE_THRESHOLD: f64 = 0.22;

/// The request's instructions, always in English like the post-processing
/// frame (`prompt_frame::FRAME_TEXT`).
pub const SYSTEM_PROMPT: &str = "You check a dictation that a speech recognizer wrote down. The speaker keeps a word list of names and terms; the recognizer doesn't know it, so it may have written such a term as similar-sounding words. For each numbered candidate, decide from the sentence whether the speaker meant the term. Confirm a candidate only when the sentence clearly means the term (a name, product or technical term); keep ordinary words that fit the sentence, even when they sound like a term. Answer with JSON only: {\"replace\": [numbers of the confirmed candidates]}, an empty list when none.";

/// The field of the answer that lists the confirmed candidates' numbers.
const REPLACE_FIELD: &str = "replace";

/// Logged once per run of the app: the check doesn't run with Apple
/// Intelligence.
static APPLE_INTELLIGENCE_LOGGED: AtomicBool = AtomicBool::new(false);

/// The words of `text` that resemble a term of the word lists in effect
/// (`word_lists::merged_vocabulary`), after a speech model that did not take
/// them as vocabulary; none when it did (`takes_vocabulary`) or when no list
/// has a term.
pub fn candidates(
    settings: &AppSettings,
    text: &str,
    takes_vocabulary: bool,
) -> Vec<WordListMatch> {
    if takes_vocabulary {
        return Vec::new();
    }
    let terms = crate::word_lists::merged_vocabulary(settings);
    if terms.is_empty() {
        return Vec::new();
    }
    word_list_candidates(text, &terms, CANDIDATE_THRESHOLD)
}

/// Whether a dictation with `settings` gets the check: the switch is on,
/// the dictation is not post-processed (`post_process`; post-processing
/// gets the candidates as a hint instead), its profile may post-process
/// (not "never", where nothing goes to a provider; variant b, 2026-11-26),
/// and post-processing has a provider and a model to ask. Not with Apple
/// Intelligence for now.
pub fn wanted(settings: &AppSettings, post_process: bool) -> bool {
    if !settings.word_list_llm_check || post_process || !settings.post_process_mode.allows() {
        return false;
    }
    let Some(provider) = settings.active_post_process_provider() else {
        debug!("No word list check: post-processing has no provider");
        return false;
    };
    if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID {
        if !APPLE_INTELLIGENCE_LOGGED.swap(true, Ordering::Relaxed) {
            info!(
                "No word list check with Apple Intelligence for now: it is asked through its own \
                 interface, not a chat request"
            );
        }
        return false;
    }
    let has_model = settings
        .post_process_models
        .get(&provider.id)
        .is_some_and(|model| !model.trim().is_empty());
    if !has_model {
        debug!(
            "No word list check: provider '{}' has no model",
            provider.id
        );
    }
    has_model
}

/// `text` on one line: a line break becomes a space, so a candidate can't
/// start a line of its own.
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// A candidate's words in the request: on one line, and a transcript tag in
/// them can't open or close one.
fn candidate_text(text: &str) -> String {
    crate::prompt_frame::defuse_named_tags(&one_line(text), "transcript")
}

/// The request's user content: the transcript in `<transcript>` tags (a tag
/// in it can't close them, as in post-processing) and the candidates, one
/// numbered line each.
fn user_content(text: &str, candidates: &[WordListMatch]) -> String {
    let lines: Vec<String> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            format!(
                "{}. \"{}\" → \"{}\"",
                index + 1,
                candidate_text(&candidate.heard),
                candidate_text(&candidate.term)
            )
        })
        .collect();
    format!(
        "{}\n\nCandidates:\n{}",
        crate::prompt_frame::tagged_transcript(text),
        lines.join("\n")
    )
}

/// The JSON schema of the answer, for a provider with structured output.
fn answer_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            (REPLACE_FIELD): {
                "type": "array",
                "items": { "type": "integer" }
            }
        },
        "required": [REPLACE_FIELD],
        "additionalProperties": false
    })
}

/// The numbers (1-based) of the candidates the answer `content` confirms,
/// each once, in its order; numbers outside `1..=count` are left out. A
/// think block before the answer and prose around its JSON object are
/// tolerated; an answer without that object is an error.
fn confirmed_numbers(content: &str, count: usize) -> Result<Vec<usize>, String> {
    let content = crate::actions::strip_think_block(content);
    let object = match (content.find('{'), content.rfind('}')) {
        (Some(start), Some(end)) if start < end => &content[start..=end],
        _ => return Err("the answer has no JSON object".to_string()),
    };
    let answer: serde_json::Value = serde_json::from_str(object)
        .map_err(|e| format!("the answer's JSON can't be read: {e}"))?;
    let Some(list) = answer
        .get(REPLACE_FIELD)
        .and_then(serde_json::Value::as_array)
    else {
        return Err(format!("the answer has no \"{REPLACE_FIELD}\" list"));
    };
    let mut numbers: Vec<usize> = Vec::new();
    for number in list
        .iter()
        .filter_map(serde_json::Value::as_u64)
        .filter_map(|number| usize::try_from(number).ok())
    {
        if (1..=count).contains(&number) && !numbers.contains(&number) {
            numbers.push(number);
        }
    }
    Ok(numbers)
}

/// Asks the post-processing provider of `settings` which `candidates` the
/// sentence `text` means, within post-processing's time limit
/// (`llm_timeouts::post_process_timeout`). The confirmed ones, in the
/// order of `candidates`; an error when the provider can't be asked or its
/// answer can't be read. Logs counts only, never text.
pub async fn check(
    settings: &AppSettings,
    text: &str,
    candidates: &[WordListMatch],
) -> Result<Vec<WordListMatch>, String> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let provider = settings
        .active_post_process_provider()
        .cloned()
        .ok_or_else(|| "post-processing has no provider".to_string())?;
    if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID {
        return Err("the word list check doesn't run with Apple Intelligence".to_string());
    }
    let model = settings
        .post_process_models
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();
    if model.trim().is_empty() {
        return Err(format!("provider '{}' has no model", provider.id));
    }
    let api_key = settings
        .post_process_api_keys
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();
    // As post-processing asks these providers to skip reasoning; a yes or
    // no per candidate needs none, so no effort either.
    let reasoning = ReasoningRequest {
        disable_by_default: matches!(provider.id.as_str(), "custom" | "openrouter"),
        effort: None,
    };
    let limit = crate::llm_timeouts::post_process_timeout(settings);
    info!(
        "Checking {} word list candidate(s) with provider '{}' (model: {})",
        candidates.len(),
        provider.id,
        model
    );
    let content = tokio::time::timeout(
        limit,
        ask(&provider, api_key, &model, text, candidates, &reasoning),
    )
    .await
    .map_err(|_| format!("no answer within {limit:?}"))??;
    let mut numbers = confirmed_numbers(&content, candidates.len())?;
    numbers.sort_unstable();
    let confirmed: Vec<WordListMatch> = numbers
        .into_iter()
        .map(|number| candidates[number - 1].clone())
        .collect();
    info!(
        "The word list check confirmed {} of {} candidate(s)",
        confirmed.len(),
        candidates.len()
    );
    Ok(confirmed)
}

/// The request, the way post-processing sends one: with structured output
/// the instructions as the system prompt and the answer's schema, falling
/// back to one message if that fails; without, one message (which also
/// reaches providers with Anthropic's API).
async fn ask(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    text: &str,
    candidates: &[WordListMatch],
    reasoning: &ReasoningRequest,
) -> Result<String, String> {
    let user = user_content(text, candidates);
    if provider.supports_structured_output {
        match crate::llm_client::send_chat_completion_with_schema(
            provider,
            api_key.clone(),
            model,
            user.clone(),
            Some(SYSTEM_PROMPT.to_string()),
            Some(answer_schema()),
            reasoning,
        )
        .await
        {
            Ok(Completion {
                content: Some(content),
                ..
            }) => return Ok(content),
            Ok(Completion { content: None, .. }) => {
                return Err("the provider's answer has no content".to_string())
            }
            Err(e) => warn!(
                "The word list check with structured output failed for provider '{}': {}; \
                 asking in one message",
                provider.id, e
            ),
        }
    }
    match crate::llm_client::send_chat_completion(
        provider,
        api_key,
        model,
        format!("{SYSTEM_PROMPT}\n\n{user}"),
        reasoning,
    )
    .await?
    {
        Completion {
            content: Some(content),
            ..
        } => Ok(content),
        Completion { content: None, .. } => Err("the provider's answer has no content".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation_flow::fakes::{llm_server, no_server};
    use crate::settings::{get_default_settings, APPLE_INTELLIGENCE_PROVIDER_ID};
    use std::time::Duration;

    fn pair(heard: &str, term: &str) -> WordListMatch {
        WordListMatch {
            heard: heard.to_string(),
            term: term.to_string(),
        }
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| word.to_string()).collect()
    }

    /// Settings that check with the default provider at `base_url`, with
    /// the model "model", structured output as `structured` says, the
    /// switch on and "DanyWhere" and "Build" in the global word list.
    fn check_settings(base_url: &str, structured: bool) -> AppSettings {
        let mut settings = get_default_settings();
        settings.word_list_llm_check = true;
        settings.custom_words = words(&["DanyWhere", "Build"]);
        let provider = settings
            .post_process_providers
            .iter_mut()
            .find(|provider| provider.id == settings.post_process_provider_id)
            .expect("the default provider exists");
        provider.base_url = base_url.to_string();
        provider.supports_structured_output = structured;
        let provider_id = provider.id.clone();
        settings
            .post_process_models
            .insert(provider_id, "model".to_string());
        settings
    }

    // ---- Candidates --------------------------------------------------------

    #[test]
    fn a_model_without_vocabulary_gets_candidates_from_the_lists_in_effect() {
        let settings = check_settings("", true);
        let found = candidates(&settings, "Ich teste Deniware und das Bild.", false);
        assert!(found.contains(&pair("Deniware", "DanyWhere")), "{found:?}");
        assert!(found.contains(&pair("Bild", "Build")), "{found:?}");
        // The active additional list counts too.
        let mut with_list = check_settings("", true);
        with_list.custom_words = Vec::new();
        with_list.word_lists = vec![crate::word_lists::WordList {
            id: "work".to_string(),
            name: "Work".to_string(),
            words: words(&["DanyWhere"]),
        }];
        with_list.active_word_list_id = Some("work".to_string());
        assert_eq!(
            candidates(&with_list, "Ich teste Deniware.", false),
            vec![pair("Deniware", "DanyWhere")]
        );
    }

    #[test]
    fn a_model_that_took_the_lists_or_no_lists_give_no_candidates() {
        let settings = check_settings("", true);
        assert!(candidates(&settings, "Ich teste Deniware.", true).is_empty());
        let mut without_terms = check_settings("", true);
        without_terms.custom_words = Vec::new();
        assert!(candidates(&without_terms, "Ich teste Deniware.", false).is_empty());
    }

    #[test]
    fn the_threshold_is_the_measured_one() {
        assert_eq!(CANDIDATE_THRESHOLD, 0.22);
    }

    // ---- Whether a dictation gets the check ------------------------------

    #[test]
    fn the_check_runs_when_switched_on_without_post_processing_and_with_a_model() {
        let settings = check_settings("http://localhost", true);
        assert!(wanted(&settings, false));
        // Post-processing gets the candidates as a hint instead.
        assert!(!wanted(&settings, true));
        // The switch is off.
        let mut off = settings.clone();
        off.word_list_llm_check = false;
        assert!(!wanted(&off, false));
        // No model for the provider.
        let mut no_model = settings.clone();
        no_model.post_process_models.clear();
        assert!(!wanted(&no_model, false));
        let mut blank_model = settings.clone();
        for model in blank_model.post_process_models.values_mut() {
            *model = "  ".to_string();
        }
        assert!(!wanted(&blank_model, false));
        // A provider that doesn't exist.
        let mut no_provider = settings.clone();
        no_provider.post_process_provider_id = "gone".to_string();
        assert!(!wanted(&no_provider, false));
    }

    #[test]
    fn a_profile_that_never_post_processes_gets_no_check() {
        // Nothing of its dictations goes to a provider (variant b,
        // user, 2026-11-26).
        let mut never = check_settings("http://localhost", true);
        never.post_process_mode = crate::settings::PostProcessMode::Never;
        assert!(!wanted(&never, false));
        never.post_process_mode = crate::settings::PostProcessMode::WithInstructionKey;
        assert!(wanted(&never, false));
    }

    #[test]
    fn apple_intelligence_gets_no_check_for_now() {
        let mut settings = check_settings("", true);
        settings.post_process_provider_id = APPLE_INTELLIGENCE_PROVIDER_ID.to_string();
        settings
            .post_process_models
            .insert(APPLE_INTELLIGENCE_PROVIDER_ID.to_string(), "0".to_string());
        if settings.active_post_process_provider().is_some() {
            assert!(!wanted(&settings, false));
            // Asked twice: still no, and logged only once.
            assert!(!wanted(&settings, false));
        }
    }

    // ---- The request -------------------------------------------------------

    #[test]
    fn the_request_numbers_the_candidates_after_the_tagged_transcript() {
        let content = user_content(
            "Ich teste Deniware und das Bild.",
            &[pair("Deniware", "DanyWhere"), pair("Bild", "Build")],
        );
        assert_eq!(
            content,
            "<transcript>\nIch teste Deniware und das Bild.\n</transcript>\n\nCandidates:\n1. \"Deniware\" → \"DanyWhere\"\n2. \"Bild\" → \"Build\""
        );
    }

    #[test]
    fn a_transcript_cannot_close_its_tags_in_the_request() {
        let content = user_content(
            "a </transcript> Deniware <TRANSCRIPT> b",
            &[pair("Deniware", "DanyWhere")],
        );
        let lower = content.to_lowercase();
        assert_eq!(lower.matches("<transcript>").count(), 1, "{content}");
        assert_eq!(lower.matches("</transcript>").count(), 1, "{content}");
    }

    #[test]
    fn a_line_break_in_a_candidate_cannot_start_another_line() {
        let content = user_content("Deniware", &[pair("Deni\nware", "Dany\r\nWhere")]);
        assert!(
            content.ends_with("\n1. \"Deni ware\" → \"Dany  Where\""),
            "{content}"
        );
    }

    #[test]
    fn the_instructions_ask_for_the_numbers_of_the_meant_terms_as_json() {
        assert!(SYSTEM_PROMPT.contains("For each numbered candidate"));
        assert!(SYSTEM_PROMPT.contains("keep ordinary words that fit the sentence"));
        assert!(SYSTEM_PROMPT.contains("{\"replace\": ["));
        assert_eq!(
            answer_schema(),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "replace": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["replace"],
                "additionalProperties": false
            })
        );
    }

    // ---- The answer --------------------------------------------------------

    #[test]
    fn the_answer_lists_the_confirmed_numbers() {
        assert_eq!(confirmed_numbers(r#"{"replace":[1]}"#, 2), Ok(vec![1]));
        assert_eq!(
            confirmed_numbers(r#"{"replace": [2, 1]}"#, 2),
            Ok(vec![2, 1])
        );
        assert_eq!(confirmed_numbers(r#"{"replace":[]}"#, 2), Ok(Vec::new()));
    }

    #[test]
    fn a_think_block_and_prose_around_the_json_are_tolerated() {
        assert_eq!(
            confirmed_numbers("<think>Bild is a word.</think>\n{\"replace\":[1]}", 2),
            Ok(vec![1])
        );
        assert_eq!(
            confirmed_numbers(
                "Here is the result:\n```json\n{\"replace\": [2]}\n```\nDone.",
                2
            ),
            Ok(vec![2])
        );
    }

    #[test]
    fn numbers_out_of_range_and_repeated_ones_are_left_out() {
        assert_eq!(
            confirmed_numbers(r#"{"replace":[0, 1, 3, 1, -2, 2, "x", 1.5]}"#, 2),
            Ok(vec![1, 2])
        );
    }

    #[test]
    fn an_answer_without_its_json_is_an_error() {
        assert!(confirmed_numbers("Yes, replace the first one.", 2).is_err());
        assert!(confirmed_numbers(r#"{"confirmed":[1]}"#, 2).is_err());
        assert!(confirmed_numbers(r#"{"replace":"1"}"#, 2).is_err());
        assert!(confirmed_numbers("", 2).is_err());
    }

    // ---- Against a provider ------------------------------------------------

    /// The messages of a request body, as (role, content).
    fn messages(request: &serde_json::Value) -> Vec<(String, String)> {
        request["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .map(|message| {
                (
                    message["role"].as_str().unwrap_or_default().to_string(),
                    message["content"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn with_structured_output_the_schema_goes_along_and_a_confirmed_pair_comes_back() {
        let (url, requests) = llm_server(vec![(
            "200 OK",
            r#"{"choices":[{"message":{"content":"{\"replace\":[1]}"}}]}"#,
        )]);
        let settings = check_settings(&url, true);
        let found = vec![pair("Deniware", "DanyWhere"), pair("Bild", "Build")];
        let text = "Ich teste Deniware und das Bild.";
        let confirmed = tauri::async_runtime::block_on(check(&settings, text, &found));
        assert_eq!(confirmed, Ok(vec![pair("Deniware", "DanyWhere")]));

        let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(request["model"], "model");
        assert_eq!(
            request["response_format"]["json_schema"]["schema"],
            answer_schema()
        );
        assert_eq!(
            messages(&request),
            vec![
                ("system".to_string(), SYSTEM_PROMPT.to_string()),
                ("user".to_string(), user_content(text, &found)),
            ]
        );
    }

    #[test]
    fn without_structured_output_one_message_asks_and_prose_around_the_answer_is_read() {
        let (url, requests) = llm_server(vec![(
            "200 OK",
            r#"{"choices":[{"message":{"content":"Sure: {\"replace\":[2]}"}}]}"#,
        )]);
        let settings = check_settings(&url, false);
        let found = vec![pair("Deniware", "DanyWhere"), pair("Bild", "Build")];
        let text = "Ich teste Deniware und das Bild.";
        let confirmed = tauri::async_runtime::block_on(check(&settings, text, &found));
        assert_eq!(confirmed, Ok(vec![pair("Bild", "Build")]));

        let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(request.get("response_format").is_none(), "{request}");
        assert_eq!(
            messages(&request),
            vec![(
                "user".to_string(),
                format!("{SYSTEM_PROMPT}\n\n{}", user_content(text, &found))
            )]
        );
    }

    #[test]
    fn a_provider_that_cannot_be_reached_or_answers_nonsense_is_an_error() {
        let found = vec![pair("Deniware", "DanyWhere")];
        let unreachable = check_settings(&no_server(), true);
        assert!(tauri::async_runtime::block_on(check(&unreachable, "Deniware", &found)).is_err());
        let (url, _requests) = llm_server(vec![(
            "200 OK",
            r#"{"choices":[{"message":{"content":"I can't tell."}}]}"#,
        )]);
        let nonsense = check_settings(&url, false);
        assert!(tauri::async_runtime::block_on(check(&nonsense, "Deniware", &found)).is_err());
    }

    #[test]
    fn without_a_provider_or_model_there_is_no_one_to_ask() {
        let found = vec![pair("Deniware", "DanyWhere")];
        let mut no_model = check_settings("http://localhost", true);
        no_model.post_process_models.clear();
        assert!(tauri::async_runtime::block_on(check(&no_model, "Deniware", &found)).is_err());
        let mut no_provider = check_settings("http://localhost", true);
        no_provider.post_process_provider_id = "gone".to_string();
        assert!(tauri::async_runtime::block_on(check(&no_provider, "Deniware", &found)).is_err());
    }

    #[test]
    fn no_candidates_need_no_request() {
        let settings = check_settings(&no_server(), true);
        assert_eq!(
            tauri::async_runtime::block_on(check(&settings, "hello", &[])),
            Ok(Vec::new())
        );
    }
}
