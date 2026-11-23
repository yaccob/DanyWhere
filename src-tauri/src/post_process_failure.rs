//! DanyWhere: why a post-processing failed, for the notice that says so
//! (`NoticeCode::PostProcessFailed`, `FilePostProcessFailed`,
//! `HistoryPostProcessFailed`). Decided by the user on 2026-11-23: the
//! notice names the actual reason, short, so the user knows what to change.
//!
//! Rust sends no text: the notice's detail is a small JSON object
//! ([`PostProcessFailure::notice_detail`]) that `src/lib/postProcessFailure.ts`
//! turns into a translated sentence after the notice's explanation. Its
//! fields: `reason` (below), `providerId` and `provider` (the provider's id
//! and stored label; the frontend translates the custom provider's name),
//! `model`, and per reason `seconds`, `status` or `error`.
//!
//! | `reason`          | when                                                        | own field |
//! |-------------------|-------------------------------------------------------------|-----------|
//! | `no-provider`     | no provider is selected                                     |           |
//! | `no-model`        | the provider has no model chosen                            |           |
//! | `no-prompt`       | no active prompt has a text                                 |           |
//! | `no-api-key`      | refused (401/403) and no API key is set                     |           |
//! | `key-refused`     | refused (401/403) with the API key set                      |           |
//! | `model-not-found` | the provider doesn't have the model (404 naming the model, `model_not_found`, "model … not found / does not exist") | |
//! | `unreachable`     | no connection: refused, unknown host, TLS                   |           |
//! | `timeout`         | no answer within the time limit                             | `seconds` |
//! | `status`          | another HTTP status                                         | `status`  |
//! | `empty-reply`     | the answer had no text                                      |           |
//! | `truncated`       | the answer was cut off at the output limit                  |           |
//! | `unavailable`     | the provider can't run here (Apple Intelligence)            |           |
//! | `other`           | anything else                                               | `error`, the raw error, shortened |
//!
//! A failure whose reason isn't known ([`FailureReason::Unknown`]) has no
//! detail, and the frontend shows what it showed before; so does a detail it
//! can't read (an older or newer one).

use crate::notices::{Notice, NoticeCode, NoticeSource};
use crate::settings::AppSettings;

/// Why a post-processing failed. See the module's table for when each one
/// is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureReason {
    NoProvider,
    NoModel,
    NoPrompt,
    NoApiKey,
    KeyRefused,
    ModelNotFound,
    Unreachable,
    /// The time limit from the settings comes with the failure
    /// ([`PostProcessFailure::new`]).
    Timeout,
    Status(u16),
    EmptyReply,
    Truncated,
    Unavailable,
    /// The raw error, shortened to [`MAX_ERROR_CHARS`].
    Other(String),
    /// No detail: the notice says what it said before this was built.
    Unknown,
}

/// The longest raw error a notice carries, in characters.
pub const MAX_ERROR_CHARS: usize = 200;

/// A failed post-processing: why, and with which provider and model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostProcessFailure {
    pub reason: FailureReason,
    /// The provider's id and stored label.
    pub provider: Option<(String, String)>,
    pub model: Option<String>,
    /// The time limit, for [`FailureReason::Timeout`].
    pub seconds: Option<u64>,
}

impl PostProcessFailure {
    /// `reason` with the provider and model `settings` post-process with,
    /// and for a timeout the time limit they set.
    pub fn new(settings: &AppSettings, reason: FailureReason) -> Self {
        let provider = settings.active_post_process_provider();
        let model = provider
            .and_then(|provider| settings.post_process_models.get(&provider.id))
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty());
        let seconds = (reason == FailureReason::Timeout)
            .then(|| crate::llm_timeouts::post_process_timeout(settings).as_secs());
        Self {
            reason,
            provider: provider.map(|provider| (provider.id.clone(), provider.label.clone())),
            model,
            seconds,
        }
    }

    /// A failure whose reason isn't known: its notice has no detail.
    pub fn unknown() -> Self {
        Self {
            reason: FailureReason::Unknown,
            provider: None,
            model: None,
            seconds: None,
        }
    }

    /// The notice's detail: a JSON object (see the module's documentation),
    /// or `None` for [`FailureReason::Unknown`].
    pub fn notice_detail(&self) -> Option<String> {
        let reason = match &self.reason {
            FailureReason::NoProvider => "no-provider",
            FailureReason::NoModel => "no-model",
            FailureReason::NoPrompt => "no-prompt",
            FailureReason::NoApiKey => "no-api-key",
            FailureReason::KeyRefused => "key-refused",
            FailureReason::ModelNotFound => "model-not-found",
            FailureReason::Unreachable => "unreachable",
            FailureReason::Timeout => "timeout",
            FailureReason::Status(_) => "status",
            FailureReason::EmptyReply => "empty-reply",
            FailureReason::Truncated => "truncated",
            FailureReason::Unavailable => "unavailable",
            FailureReason::Other(_) => "other",
            FailureReason::Unknown => return None,
        };
        let mut detail = serde_json::Map::new();
        detail.insert("reason".to_string(), reason.into());
        if let Some((id, label)) = &self.provider {
            detail.insert("providerId".to_string(), id.as_str().into());
            detail.insert("provider".to_string(), label.as_str().into());
        }
        if let Some(model) = &self.model {
            detail.insert("model".to_string(), model.as_str().into());
        }
        match &self.reason {
            FailureReason::Timeout => {
                if let Some(seconds) = self.seconds {
                    detail.insert("seconds".to_string(), seconds.into());
                }
            }
            FailureReason::Status(status) => {
                detail.insert("status".to_string(), (*status).into());
            }
            FailureReason::Other(error) => {
                detail.insert("error".to_string(), error.as_str().into());
            }
            _ => {}
        }
        Some(serde_json::Value::Object(detail).to_string())
    }

    /// The notice `code` from `source` that says this failure's reason.
    pub fn notice(&self, code: NoticeCode, source: NoticeSource) -> Notice {
        Notice::new(code, source).with_detail(self.notice_detail())
    }
}

/// Why a request of `llm_client` failed, from its error text: the status
/// and body of an error answer (`llm_client::STATUS_ERROR_PREFIX`), the
/// kinds `llm_client::report_reqwest_error` names, or a truncated answer
/// (`llm_client::TRUNCATED_ERROR_PREFIX`). `api_key_set`: whether the
/// request carried an API key, which tells a missing key from a refused one.
pub fn classify_llm_error(error: &str, api_key_set: bool) -> FailureReason {
    if let Some(answer) = error.strip_prefix(crate::llm_client::STATUS_ERROR_PREFIX) {
        // "404 Not Found: {body}"
        let digits: String = answer.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(status) = digits.parse::<u16>() {
            let body = answer.split_once(": ").map_or("", |(_, body)| body);
            if names_a_missing_model(status, body) {
                return FailureReason::ModelNotFound;
            }
            if matches!(status, 401 | 403) {
                return if api_key_set {
                    FailureReason::KeyRefused
                } else {
                    FailureReason::NoApiKey
                };
            }
            return FailureReason::Status(status);
        }
    }
    if error.starts_with(crate::llm_client::TRUNCATED_ERROR_PREFIX) {
        return FailureReason::Truncated;
    }
    let kinds = transport_kinds(error);
    if kinds.contains(&"connect") {
        return FailureReason::Unreachable;
    }
    if kinds.contains(&"timeout") {
        return FailureReason::Timeout;
    }
    FailureReason::other(error)
}

impl FailureReason {
    /// [`FailureReason::Other`] with `error`, shortened to
    /// [`MAX_ERROR_CHARS`].
    pub fn other(error: &str) -> Self {
        let error = error.trim();
        if error.chars().count() <= MAX_ERROR_CHARS {
            return Self::Other(error.to_string());
        }
        let mut short: String = error.chars().take(MAX_ERROR_CHARS - 1).collect();
        short.push('…');
        Self::Other(short)
    }
}

/// Whether an error answer says the provider doesn't have the model:
/// OpenAI and the providers that answer as it does say `model_not_found`
/// (also with 400 or 403), Ollama "model '…' not found", Anthropic a 404
/// about "model: …", OpenRouter "… is not a valid model ID". A 404 that
/// names no model is a wrong address, not a missing model.
fn names_a_missing_model(status: u16, body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    body.contains("model_not_found")
        || (body.contains("model")
            && (status == 404
                || body.contains("not found")
                || body.contains("does not exist")
                || body.contains("not a valid model")))
}

/// The kinds `llm_client::report_reqwest_error` names in its error text,
/// "… (kind: connect, request, url: …): …".
fn transport_kinds(error: &str) -> Vec<&str> {
    const MARKER: &str = "(kind: ";
    let Some(start) = error.find(MARKER) else {
        return Vec::new();
    };
    let rest = &error[start + MARKER.len()..];
    let end = [rest.find(", url: "), rest.find(')')]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(rest.len());
    rest[..end].split(", ").collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;
    use serde_json::{json, Value};

    const OLLAMA_404: &str = r#"API request failed with status 404 Not Found: {"error":{"message":"model 'gemma4-12b-qat' not found","type":"not_found_error","param":null,"code":null}}"#;
    const OPENAI_404: &str = r#"API request failed with status 404 Not Found: {"error":{"message":"The model `gpt-9` does not exist or you do not have access to it.","type":"invalid_request_error","param":null,"code":"model_not_found"}}"#;
    const ANTHROPIC_404: &str = r#"API request failed with status 404 Not Found: {"type":"error","error":{"type":"not_found_error","message":"model: claude-nothing"}}"#;
    const OPENROUTER_400: &str = r#"API request failed with status 400 Bad Request: {"error":{"message":"nothing/model is not a valid model ID","code":400}}"#;
    const WRONG_PATH_404: &str = "API request failed with status 404 Not Found: 404 page not found";
    const KEY_401: &str = r#"API request failed with status 401 Unauthorized: {"error":{"message":"Incorrect API key provided: sk-****. You can find your API key at https://platform.openai.com/account/api-keys.","type":"invalid_request_error","param":null,"code":"invalid_api_key"}}"#;
    const FORBIDDEN_403: &str = "API request failed with status 403 Forbidden: forbidden";
    const SERVER_500: &str =
        "API request failed with status 500 Internal Server Error: something broke";
    const RATE_429: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"message":"You exceeded your current quota","code":"insufficient_quota"}}"#;
    const REFUSED: &str = "HTTP request failed (kind: request, connect, url: http://127.0.0.1:9/chat/completions): caused by: error sending request -> client error (Connect) -> tcp connect error -> Connection refused (os error 61)";
    const UNKNOWN_HOST: &str = "HTTP request failed (kind: request, connect, url: https://no-such-host.invalid/v1/chat/completions): caused by: error sending request -> client error (Connect) -> dns error -> failed to lookup address information: nodename nor servname provided, or not known";
    const TIMED_OUT: &str = "HTTP request failed (kind: request, timeout, url: http://127.0.0.1:9/chat/completions): caused by: operation timed out";
    const TRUNCATED: &str = "Response was truncated at the provider's output token limit";
    const ANTHROPIC_TRUNCATED: &str = "Response was truncated at the 4096 token limit";

    #[test]
    fn a_model_the_provider_doesn_t_have_is_named_as_such() {
        for error in [OLLAMA_404, OPENAI_404, ANTHROPIC_404, OPENROUTER_400] {
            assert_eq!(
                classify_llm_error(error, true),
                FailureReason::ModelNotFound,
                "{error}"
            );
        }
        // Also without a key: the provider answered about the model.
        assert_eq!(
            classify_llm_error(OLLAMA_404, false),
            FailureReason::ModelNotFound
        );
    }

    #[test]
    fn a_404_that_names_no_model_is_its_status() {
        assert_eq!(
            classify_llm_error(WRONG_PATH_404, false),
            FailureReason::Status(404)
        );
    }

    #[test]
    fn a_refusal_is_a_missing_key_without_one_and_a_refused_key_with_one() {
        assert_eq!(classify_llm_error(KEY_401, false), FailureReason::NoApiKey);
        assert_eq!(classify_llm_error(KEY_401, true), FailureReason::KeyRefused);
        assert_eq!(
            classify_llm_error(FORBIDDEN_403, true),
            FailureReason::KeyRefused
        );
        assert_eq!(
            classify_llm_error(FORBIDDEN_403, false),
            FailureReason::NoApiKey
        );
    }

    #[test]
    fn another_status_is_named_by_its_number() {
        assert_eq!(
            classify_llm_error(SERVER_500, true),
            FailureReason::Status(500)
        );
        assert_eq!(
            classify_llm_error(RATE_429, true),
            FailureReason::Status(429)
        );
    }

    #[test]
    fn no_connection_is_unreachable_and_no_answer_a_timeout() {
        assert_eq!(
            classify_llm_error(REFUSED, false),
            FailureReason::Unreachable
        );
        assert_eq!(
            classify_llm_error(UNKNOWN_HOST, true),
            FailureReason::Unreachable
        );
        assert_eq!(classify_llm_error(TIMED_OUT, true), FailureReason::Timeout);
    }

    #[test]
    fn a_cut_off_answer_is_truncated() {
        assert_eq!(
            classify_llm_error(TRUNCATED, true),
            FailureReason::Truncated
        );
        assert_eq!(
            classify_llm_error(ANTHROPIC_TRUNCATED, true),
            FailureReason::Truncated
        );
    }

    #[test]
    fn anything_else_keeps_its_raw_error_shortened() {
        assert_eq!(
            classify_llm_error("Invalid authorization header value: failed to parse", true),
            FailureReason::Other("Invalid authorization header value: failed to parse".to_string())
        );
        let long = "x".repeat(MAX_ERROR_CHARS + 50);
        match classify_llm_error(&long, true) {
            FailureReason::Other(error) => {
                assert_eq!(error.chars().count(), MAX_ERROR_CHARS);
                assert!(error.ends_with('…'));
            }
            other => panic!("expected Other, got {other:?}"),
        }
    }

    /// The default settings with provider "custom" (Ollama) and `model`.
    fn custom_settings(model: &str) -> AppSettings {
        let mut settings = get_default_settings();
        settings.post_process_provider_id = "custom".to_string();
        settings
            .post_process_models
            .insert("custom".to_string(), model.to_string());
        settings
    }

    fn detail_of(failure: &PostProcessFailure) -> Value {
        let detail = failure.notice_detail().expect("a detail");
        serde_json::from_str(&detail).expect("the detail is JSON")
    }

    #[test]
    fn the_detail_names_the_reason_the_provider_and_the_model() {
        let failure = PostProcessFailure::new(
            &custom_settings("gemma4-12b-qat"),
            FailureReason::ModelNotFound,
        );
        assert_eq!(
            detail_of(&failure),
            json!({
                "reason": "model-not-found",
                "providerId": "custom",
                "provider": "Custom",
                "model": "gemma4-12b-qat",
            })
        );
    }

    #[test]
    fn the_detail_carries_what_its_reason_needs() {
        let mut settings = custom_settings("m");
        settings.post_process_timeout_secs = 45;
        assert_eq!(
            detail_of(&PostProcessFailure::new(&settings, FailureReason::Timeout))["seconds"],
            json!(45)
        );
        assert_eq!(
            detail_of(&PostProcessFailure::new(
                &settings,
                FailureReason::Status(503)
            ))["status"],
            json!(503)
        );
        assert_eq!(
            detail_of(&PostProcessFailure::new(
                &settings,
                FailureReason::Other("boom".to_string())
            ))["error"],
            json!("boom")
        );
        let reasons = [
            (FailureReason::NoModel, "no-model"),
            (FailureReason::NoPrompt, "no-prompt"),
            (FailureReason::NoApiKey, "no-api-key"),
            (FailureReason::KeyRefused, "key-refused"),
            (FailureReason::Unreachable, "unreachable"),
            (FailureReason::EmptyReply, "empty-reply"),
            (FailureReason::Truncated, "truncated"),
            (FailureReason::Unavailable, "unavailable"),
        ];
        for (reason, name) in reasons {
            let detail = detail_of(&PostProcessFailure::new(&settings, reason));
            assert_eq!(detail["reason"], json!(name));
            assert_eq!(detail["provider"], json!("Custom"));
            assert_eq!(detail.get("seconds"), None, "{name}");
        }
    }

    #[test]
    fn without_a_provider_or_model_the_detail_leaves_them_out() {
        let mut settings = get_default_settings();
        settings.post_process_provider_id = "gone".to_string();
        assert_eq!(
            detail_of(&PostProcessFailure::new(
                &settings,
                FailureReason::NoProvider
            )),
            json!({ "reason": "no-provider" })
        );
        let settings = custom_settings("  ");
        assert_eq!(
            detail_of(&PostProcessFailure::new(&settings, FailureReason::NoModel)),
            json!({ "reason": "no-model", "providerId": "custom", "provider": "Custom" })
        );
    }

    #[test]
    fn an_unknown_reason_has_no_detail() {
        assert_eq!(PostProcessFailure::unknown().notice_detail(), None);
        assert_eq!(
            PostProcessFailure::unknown()
                .notice(NoticeCode::PostProcessFailed, NoticeSource::Dictation),
            Notice::new(NoticeCode::PostProcessFailed, NoticeSource::Dictation)
        );
    }

    #[test]
    fn the_notice_carries_the_detail() {
        let failure = PostProcessFailure::new(
            &custom_settings("gemma4-12b-qat"),
            FailureReason::ModelNotFound,
        );
        let notice = failure.notice(NoticeCode::FilePostProcessFailed, NoticeSource::File);
        assert_eq!(notice.code, NoticeCode::FilePostProcessFailed);
        assert_eq!(notice.detail, failure.notice_detail());
        assert!(notice.detail.is_some());
    }
}
