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
//! | `no-credit`       | the account has no credit left (402, Anthropic's "credit balance is too low", OpenAI's `insufficient_quota`, "insufficient balance") | |
//! | `free-daily-limit`| OpenRouter's free models: the day's requests are used up (`free-models-per-day`) | |
//! | `key-limit`       | OpenRouter: the key's own credit limit is used up (`limit_source` `openrouter_key_limit`) | |
//! | `free-busy`       | OpenRouter: the free model is busy at its provider (429 "rate-limited upstream" of a `:free` model, `upstream_provider_shared_pool`) | |
//! | `plan-limit`      | ChatGPT: the plan's usage limit is reached (`subscription_sharing_usage_limit_exceeded`) | |
//! | `plan-not-eligible` | ChatGPT: the plan can't be used by apps (`subscription_sharing_user_not_eligible`, an error starting with `plan-not-eligible:`) | |
//! | `sign-in-expired` | the sign-in is no longer valid (an error starting with `sign-in-expired:`, ChatGPT's `subscription_sharing_invalid_user`) | |
//! | `rate-limited`    | another 429: too many requests or tokens for now (Groq's tokens per minute, OpenAI's and Anthropic's rate limits) | |
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

use std::sync::OnceLock;

use regex::Regex;

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
    /// The provider's account has no credit left.
    NoCredit,
    /// OpenRouter's free models: the day's requests are used up.
    FreeDailyLimit,
    /// OpenRouter: the key's own credit limit is used up.
    KeyLimit,
    /// OpenRouter: the free model is busy at its provider right now.
    FreeBusy,
    /// ChatGPT: the plan's usage limit is reached.
    PlanLimit,
    /// ChatGPT: the plan can't be used by apps.
    PlanNotEligible,
    /// The sign-in (ChatGPT) is no longer valid.
    SignInExpired,
    /// Too many requests or tokens for now; waiting helps.
    RateLimited,
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

/// How the error of a sign-in that is no longer valid begins (a refresh
/// token the provider refuses, e.g. ChatGPT's `invalid_grant`); the
/// provider's code follows.
pub const SIGN_IN_EXPIRED_PREFIX: &str = "sign-in-expired:";

/// How the error of a sign-in begins whose plan apps may not use (strand
/// C's `ChatGptError::NotEligible`).
pub const PLAN_NOT_ELIGIBLE_PREFIX: &str = "plan-not-eligible:";

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
        let reason = self.reason.name()?;
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
///
/// DanyWhere (the post-processing setup, 2026-11-26): no credit stays the
/// first reason of an error answer; then ChatGPT's plan, OpenRouter's limits
/// (the free models' day, the key's own limit, a busy free model), the
/// missing model and the refused key, and a 429 left over is a rate limit.
/// Each looks for its own marker, so none takes another's answer.
pub fn classify_llm_error(error: &str, api_key_set: bool) -> FailureReason {
    if error.trim_start().starts_with(SIGN_IN_EXPIRED_PREFIX) {
        return FailureReason::SignInExpired;
    }
    if error.trim_start().starts_with(PLAN_NOT_ELIGIBLE_PREFIX) {
        return FailureReason::PlanNotEligible;
    }
    if let Some(answer) = error.strip_prefix(crate::llm_client::STATUS_ERROR_PREFIX) {
        // "404 Not Found: {body}"
        let digits: String = answer.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(status) = digits.parse::<u16>() {
            let body = answer.split_once(": ").map_or("", |(_, body)| body);
            if says_no_credit(status, body) {
                return FailureReason::NoCredit;
            }
            if let Some(reason) = chatgpt_plan_reason(body) {
                return reason;
            }
            if body.contains(FREE_DAILY_LIMIT_MARKER) {
                return FailureReason::FreeDailyLimit;
            }
            if body.contains(KEY_LIMIT_MARKER) {
                return FailureReason::KeyLimit;
            }
            if says_free_model_busy(status, body) {
                return FailureReason::FreeBusy;
            }
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
            if status == 429 {
                return FailureReason::RateLimited;
            }
            return FailureReason::Status(status);
        }
    }
    // A failed stream carries ChatGPT's code without a status.
    if let Some(reason) = chatgpt_plan_reason(error) {
        return reason;
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
    /// The reason's name in a notice's detail (the module's table), or
    /// `None` for [`FailureReason::Unknown`].
    pub fn name(&self) -> Option<&'static str> {
        Some(match self {
            FailureReason::NoProvider => "no-provider",
            FailureReason::NoModel => "no-model",
            FailureReason::NoPrompt => "no-prompt",
            FailureReason::NoApiKey => "no-api-key",
            FailureReason::KeyRefused => "key-refused",
            FailureReason::NoCredit => "no-credit",
            FailureReason::FreeDailyLimit => "free-daily-limit",
            FailureReason::KeyLimit => "key-limit",
            FailureReason::FreeBusy => "free-busy",
            FailureReason::PlanLimit => "plan-limit",
            FailureReason::PlanNotEligible => "plan-not-eligible",
            FailureReason::SignInExpired => "sign-in-expired",
            FailureReason::RateLimited => "rate-limited",
            FailureReason::ModelNotFound => "model-not-found",
            FailureReason::Unreachable => "unreachable",
            FailureReason::Timeout => "timeout",
            FailureReason::Status(_) => "status",
            FailureReason::EmptyReply => "empty-reply",
            FailureReason::Truncated => "truncated",
            FailureReason::Unavailable => "unavailable",
            FailureReason::Other(_) => "other",
            FailureReason::Unknown => return None,
        })
    }

    /// [`FailureReason::Other`] with `error`, shortened to
    /// [`MAX_ERROR_CHARS`].
    pub fn other(error: &str) -> Self {
        let error = redact_secrets(error.trim());
        let error = error.as_str();
        if error.chars().count() <= MAX_ERROR_CHARS {
            return Self::Other(error.to_string());
        }
        let mut short: String = error.chars().take(MAX_ERROR_CHARS - 1).collect();
        short.push('…');
        Self::Other(short)
    }
}

/// `text` (an error, e.g. a provider's answer) without what identifies the
/// account or its key: the value of every `user_id` (OpenRouter puts the
/// account's id into its error answers; strand L, 2026-11-26) and OpenRouter
/// key fragments (`sk-or-…`). For a notice's detail and the log.
pub fn redact_secrets(text: &str) -> String {
    static USER_ID: OnceLock<Option<Regex>> = OnceLock::new();
    static OPENROUTER_KEY: OnceLock<Option<Regex>> = OnceLock::new();
    // Also inside a provider's raw answer quoted as a string (`\"user_id\"`).
    let user_id = USER_ID
        .get_or_init(|| Regex::new(r#"(\\?"user_id\\?"\s*:\s*\\?")[^"\\]*"#).ok())
        .as_ref();
    let key = OPENROUTER_KEY
        .get_or_init(|| Regex::new(r"sk-or-[A-Za-z0-9_\-]+").ok())
        .as_ref();
    let text = match user_id {
        Some(pattern) => pattern.replace_all(text, "${1}[REDACTED]").into_owned(),
        None => text.to_string(),
    };
    match key {
        Some(pattern) => pattern.replace_all(&text, "sk-or-[REDACTED]").into_owned(),
        None => text,
    }
}

/// OpenRouter's daily limit of the free models ("Limits" page: 50 requests
/// a day below 10 credits bought, 1000 from then on).
const FREE_DAILY_LIMIT_MARKER: &str = "free-models-per-day";

/// OpenRouter's limit of the key itself: "`openrouter_key_limit` means the
/// API key's credit limit is exhausted" ("Limits" page).
const KEY_LIMIT_MARKER: &str = "openrouter_key_limit";

/// ChatGPT's codes for the plan ("Sign in with ChatGPT", errors and
/// recovery; the trial's script `…_chatgpt-anmeldung.py`): the usage limit
/// (429), a plan apps may not use (403), and a sign-in no longer valid
/// (401, "sign in again after a confirmed revocation").
fn chatgpt_plan_reason(text: &str) -> Option<FailureReason> {
    if text.contains("subscription_sharing_usage_limit_exceeded") {
        Some(FailureReason::PlanLimit)
    } else if text.contains("subscription_sharing_user_not_eligible") {
        Some(FailureReason::PlanNotEligible)
    } else if text.contains("subscription_sharing_invalid_user") {
        Some(FailureReason::SignInExpired)
    } else {
        None
    }
}

/// Whether a 429 says a free model of OpenRouter is busy at its provider
/// (2026-11-26: "google/gemma-4-26b-a4b-it:free is temporarily rate-limited
/// upstream", `limit_source` `upstream_provider_shared_pool`, without
/// `retry-after`): the free models share their provider's pool with
/// everyone. A paid model limited upstream is a rate limit.
fn says_free_model_busy(status: u16, body: &str) -> bool {
    status == 429
        && (body.contains("upstream_provider_shared_pool")
            || (body.contains("rate-limited upstream") && body.contains(":free")))
}

/// Whether an error answer says the account has no credit left (user,
/// 2026-11-26, after Anthropic's answer showed only as "HTTP status 400"):
/// every 402 Payment Required (OpenRouter, DeepSeek), Anthropic's 400
/// "credit balance is too low", OpenAI's `insufficient_quota` (a 429, unlike
/// its rate limits) and "insufficient balance" (z.ai). Checked before the
/// other reasons, since topping up is what helps. DanyWhere (2026-11-26):
/// not a 402 that names OpenRouter's limit of the free models' day or of
/// the key itself, which have reasons of their own.
fn says_no_credit(status: u16, body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    if body.contains(FREE_DAILY_LIMIT_MARKER) || body.contains(KEY_LIMIT_MARKER) {
        return false;
    }
    status == 402
        || body.contains("credit balance is too low")
        || body.contains("insufficient_quota")
        || body.contains("insufficient credits")
        || body.contains("insufficient balance")
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
    const RATE_429: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"message":"Rate limit reached for gpt-5 in organization org-x on tokens per min (TPM): Limit 30000, Used 30000, Requested 900. Please try again in 1.8s.","type":"tokens","param":null,"code":"rate_limit_exceeded"}}"#;
    const ANTHROPIC_RATE_429: &str = r#"API request failed with status 429 Too Many Requests: {"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed the rate limit for your organization of 50 requests per minute."}}"#;
    /// The answer DanyWhere's log showed on 2026-11-26, word for word.
    const ANTHROPIC_NO_CREDIT: &str = r#"API request failed with status 400 Bad Request: {"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits."},"request_id":"req_011ChRweqaxNgJyv6HyXKRtY"}"#;
    const OPENAI_NO_CREDIT: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"message":"You exceeded your current quota, please check your plan and billing details. For more information on this error, read the docs: https://platform.openai.com/docs/guides/error-codes/api-errors.","type":"insufficient_quota","param":null,"code":"insufficient_quota"}}"#;
    const OPENROUTER_402: &str = r#"API request failed with status 402 Payment Required: {"error":{"message":"Insufficient credits. Add more using https://openrouter.ai/settings/credits","code":402}}"#;
    const ZAI_NO_BALANCE: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"code":"1113","message":"Insufficient balance or no resource package. Please recharge."}}"#;
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
    }

    // ---- The reasons of the post-processing setup (2026-11-26) -------------
    //
    // Wordings: the trial of 2026-11-26 (sidecar note
    // `2026-11-26_1955_nachbearbeitung-versuche`, sections 3 and 4, its
    // results word for word, an organization id replaced), OpenRouter's
    // "Limits" and "Errors and Debugging" pages and OpenAI's error codes for
    // "Sign in with ChatGPT" (the trial's script `…_chatgpt-anmeldung.py`).

    /// OpenRouter, 2026-11-26 20:19: a free Gemma model, word for word up to
    /// where the trial's record ends (it keeps 400 characters), then closed.
    const OPENROUTER_FREE_BUSY: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"message":"Provider returned error","code":429,"metadata":{"raw":"google/gemma-4-26b-a4b-it:free is temporarily rate-limited upstream. Please retry shortly, or add your own provider key to use your own limits: https://openrouter.ai/workspaces/default/byok","provider_name":"Google AI Studio","is_byok":false,"provider_error_code":"429","limit_source":"upstream_provider_shared_pool"}}}"#;
    /// OpenRouter's daily limit of the free models (50 a day below 10
    /// credits bought, "Limits" page), as clients quote it.
    const OPENROUTER_FREE_DAILY: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"message":"Rate limit exceeded: free-models-per-day. Add 10 credits to unlock 1000 free model requests per day","code":429,"metadata":{"headers":{"X-RateLimit-Limit":"50","X-RateLimit-Remaining":"0","X-RateLimit-Reset":"1795392000000"},"provider_name":null}}}"#;
    /// The same limit as a 402, as one client reports it.
    const OPENROUTER_FREE_DAILY_402: &str = r#"API request failed with status 402 Payment Required: {"error":{"message":"Rate limit exceeded: free-models-per-day. Add 10 credits to unlock 1000 free model requests per day","code":402}}"#;
    /// OpenRouter's key limit: "`openrouter_key_limit` means the API key's
    /// credit limit is exhausted" (402 on the "Limits" page; its section on
    /// budgets names a 403). The message isn't documented; the
    /// `limit_source` is what counts.
    const OPENROUTER_KEY_LIMIT: &str = r#"API request failed with status 402 Payment Required: {"error":{"code":402,"message":"Key limit exceeded","metadata":{"error_type":"payment_required","limit_source":"openrouter_key_limit","limit_remaining":0}}}"#;
    const OPENROUTER_KEY_LIMIT_403: &str = r#"API request failed with status 403 Forbidden: {"error":{"code":403,"message":"Key limit exceeded","metadata":{"limit_source":"openrouter_key_limit"}}}"#;
    /// OpenRouter's own rate limit ("Errors and Debugging", word for word).
    const OPENROUTER_RATE_429: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"code":429,"message":"Rate limit exceeded","metadata":{"error_type":"rate_limit_exceeded"}}}"#;
    /// Groq, 2026-11-26 20:52, word for word but the organization id.
    const GROQ_TPM: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"message":"Rate limit reached for model `openai/gpt-oss-120b` in organization `org_test` service tier `on_demand` on tokens per minute (TPM): Limit 8000, Used 5959, Requested 3039. Please try again in 7.485s. Need more tokens? Upgrade to Dev Tier today at https://console.groq.com/settings/billing","type":"tokens","code":"rate_limit_exceeded"}}"#;
    /// ChatGPT's plan limit, as an error answer and as a failed stream
    /// (`response.failed`, the trial script's test).
    const CHATGPT_PLAN_LIMIT: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"code":"subscription_sharing_usage_limit_exceeded","message":"limit"}}"#;
    const CHATGPT_PLAN_LIMIT_STREAM: &str = r#"Responses stream failed: {"code":"subscription_sharing_usage_limit_exceeded","message":"limit"}"#;
    const CHATGPT_NOT_ELIGIBLE: &str = r#"API request failed with status 403 Forbidden: {"error":{"code":"subscription_sharing_user_not_eligible","message":"not eligible"}}"#;
    const CHATGPT_INVALID_USER: &str = r#"API request failed with status 401 Unauthorized: {"error":{"code":"subscription_sharing_invalid_user","message":"invalid user"}}"#;

    #[test]
    fn openrouters_free_models_say_busy_or_used_up_for_the_day() {
        assert_eq!(
            classify_llm_error(OPENROUTER_FREE_BUSY, true),
            FailureReason::FreeBusy
        );
        // The day's limit, also as a 402: not "no credit".
        for error in [OPENROUTER_FREE_DAILY, OPENROUTER_FREE_DAILY_402] {
            assert_eq!(
                classify_llm_error(error, true),
                FailureReason::FreeDailyLimit,
                "{error}"
            );
        }
    }

    #[test]
    fn openrouters_key_limit_is_not_the_accounts_credit() {
        for error in [OPENROUTER_KEY_LIMIT, OPENROUTER_KEY_LIMIT_403] {
            assert_eq!(
                classify_llm_error(error, true),
                FailureReason::KeyLimit,
                "{error}"
            );
        }
        // OpenRouter's account without credit stays "no credit".
        assert_eq!(
            classify_llm_error(OPENROUTER_402, true),
            FailureReason::NoCredit
        );
    }

    #[test]
    fn a_rate_limit_says_wait() {
        for error in [RATE_429, ANTHROPIC_RATE_429, GROQ_TPM, OPENROUTER_RATE_429] {
            assert_eq!(
                classify_llm_error(error, true),
                FailureReason::RateLimited,
                "{error}"
            );
        }
        // A 429 that says the credit is gone stays "no credit".
        for error in [OPENAI_NO_CREDIT, ZAI_NO_BALANCE] {
            assert_eq!(
                classify_llm_error(error, true),
                FailureReason::NoCredit,
                "{error}"
            );
        }
    }

    #[test]
    fn the_chatgpt_plan_says_its_limit_and_whether_it_may_be_used() {
        for error in [CHATGPT_PLAN_LIMIT, CHATGPT_PLAN_LIMIT_STREAM] {
            assert_eq!(
                classify_llm_error(error, true),
                FailureReason::PlanLimit,
                "{error}"
            );
        }
        // A 403, but not a refused key.
        assert_eq!(
            classify_llm_error(CHATGPT_NOT_ELIGIBLE, true),
            FailureReason::PlanNotEligible
        );
        // Strand C's sign-in names it so when the token exchange says it.
        assert_eq!(
            classify_llm_error(
                "plan-not-eligible: this ChatGPT plan can't be used by apps",
                true
            ),
            FailureReason::PlanNotEligible
        );
    }

    #[test]
    fn an_expired_sign_in_asks_to_sign_in_again() {
        assert_eq!(
            classify_llm_error("sign-in-expired: invalid_grant", true),
            FailureReason::SignInExpired
        );
        assert_eq!(
            classify_llm_error(
                &format!("{SIGN_IN_EXPIRED_PREFIX} refresh_token_expired"),
                false
            ),
            FailureReason::SignInExpired
        );
        // A 401, but no missing key: the sign-in has to be renewed.
        assert_eq!(
            classify_llm_error(CHATGPT_INVALID_USER, true),
            FailureReason::SignInExpired
        );
    }

    /// OpenRouter's answers carry the account's id (strand L, 2026-11-26).
    const WITH_USER_ID: &str = r#"Responses stream failed: {"error":{"message":"Unexpected upstream answer","code":500,"metadata":{"provider_name":"Chutes"}},"user_id":"user_2xYz0123456789AbCdEf"}"#;

    #[test]
    fn the_accounts_id_and_key_never_reach_the_notice() {
        let FailureReason::Other(error) = classify_llm_error(WITH_USER_ID, true) else {
            panic!("expected Other");
        };
        assert!(!error.contains("user_2xYz0123456789AbCdEf"), "{error}");
        assert!(error.contains("user_id"), "the field stays: {error}");
        // Also escaped inside a provider's raw answer, and a key fragment.
        let redacted = redact_secrets(
            r#"{"metadata":{"raw":"{\"user_id\":\"user_2abc\",\"key\":\"sk-or-v1-0123456789abcdef\"}"},"user_id": "user_2def"}"#,
        );
        assert!(!redacted.contains("user_2abc"), "{redacted}");
        assert!(!redacted.contains("user_2def"), "{redacted}");
        assert!(!redacted.contains("0123456789abcdef"), "{redacted}");
        assert!(redacted.contains("sk-or-"), "{redacted}");
        // Nothing else changes.
        assert_eq!(redact_secrets(GROQ_TPM), GROQ_TPM);
        // A busy free model with the account's id is still busy.
        let busy = OPENROUTER_FREE_BUSY.replace(
            r#""limit_source""#,
            r#""user_id":"user_2xYz","limit_source""#,
        );
        assert_eq!(classify_llm_error(&busy, true), FailureReason::FreeBusy);
    }

    #[test]
    fn the_new_reasons_have_their_names() {
        let names = [
            (FailureReason::FreeDailyLimit, "free-daily-limit"),
            (FailureReason::KeyLimit, "key-limit"),
            (FailureReason::FreeBusy, "free-busy"),
            (FailureReason::PlanLimit, "plan-limit"),
            (FailureReason::PlanNotEligible, "plan-not-eligible"),
            (FailureReason::SignInExpired, "sign-in-expired"),
            (FailureReason::RateLimited, "rate-limited"),
        ];
        for (reason, name) in names {
            assert_eq!(reason.name(), Some(name));
        }
        assert_eq!(FailureReason::Unknown.name(), None);
    }

    #[test]
    fn a_used_up_credit_is_named_as_such() {
        // Anthropic's 400 (user, 2026-11-26: "keine Ahnung warum"), OpenAI's
        // 429 `insufficient_quota`, OpenRouter's 402, z.ai's 429.
        for error in [
            ANTHROPIC_NO_CREDIT,
            OPENAI_NO_CREDIT,
            OPENROUTER_402,
            ZAI_NO_BALANCE,
        ] {
            assert_eq!(
                classify_llm_error(error, true),
                FailureReason::NoCredit,
                "{error}"
            );
        }
        // Every 402 Payment Required, whatever its body says.
        assert_eq!(
            classify_llm_error(
                "API request failed with status 402 Payment Required: payment required",
                true
            ),
            FailureReason::NoCredit
        );
        // The provider took the key to look at the account: not a missing key.
        assert_eq!(
            classify_llm_error(ANTHROPIC_NO_CREDIT, false),
            FailureReason::NoCredit
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
            (FailureReason::NoCredit, "no-credit"),
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
