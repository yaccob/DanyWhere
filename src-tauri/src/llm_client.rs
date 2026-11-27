use crate::settings::{PostProcessApiFormat, PostProcessProvider};
use log::{debug, error, info};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Debug, Serialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize, Clone)]
struct JsonSchema {
    name: String,
    strict: bool,
    schema: Value,
}

#[derive(Debug, Serialize, Clone)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
    json_schema: JsonSchema,
}

#[derive(Debug, Serialize, Clone, Default, PartialEq)]
struct ReasoningConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude: Option<bool>,
}

/// Request fields used to ask an endpoint to skip reasoning/thinking, or
/// (DanyWhere) to think with a chosen effort. Providers disagree on the field
/// name and accepted values, so at most one of these is set per request (see
/// `reasoning_disable_params` and `effort_params`).
#[derive(Debug, Serialize, Clone, Default, PartialEq)]
struct ReasoningParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Value>,
}

/// Pick the reasoning-disable request fields an endpoint understands.
/// Unknown endpoints get the common OpenAI-style field; if they reject it,
/// the request is retried without it (see `send_chat_completion_with_schema`).
fn reasoning_disable_params(provider: &PostProcessProvider) -> ReasoningParams {
    let base_url = provider.base_url.to_lowercase();
    if base_url.contains("api.deepseek.com") {
        // DeepSeek rejects reasoning_effort "none" and uses its own field:
        // https://api-docs.deepseek.com/guides/thinking_mode
        ReasoningParams {
            thinking: Some(serde_json::json!({ "type": "disabled" })),
            ..Default::default()
        }
    } else if provider.id == "openrouter" {
        // OpenRouter nested object; exclude:true also keeps reasoning text out
        // of the response so it can't pollute structured-output JSON parsing
        ReasoningParams {
            reasoning: Some(ReasoningConfig {
                effort: Some("none".to_string()),
                exclude: Some(true),
            }),
            ..Default::default()
        }
    } else {
        ReasoningParams {
            reasoning_effort: Some("none".to_string()),
            ..Default::default()
        }
    }
}

/// DanyWhere: the request fields for an effort (`post_process_effort`).
/// OpenRouter nests it like the reasoning-disable fields; every other
/// OpenAI-compatible endpoint DanyWhere knows (OpenAI, Groq, Cerebras, Z.AI,
/// DeepSeek, Bedrock Mantle, Ollama) takes the top-level `reasoning_effort`.
fn effort_params(provider: &PostProcessProvider, level: &str) -> ReasoningParams {
    if provider.id == "openrouter" {
        // exclude:true keeps the reasoning text out of the response, as for
        // the reasoning-disable fields.
        ReasoningParams {
            reasoning: Some(ReasoningConfig {
                effort: Some(level.to_string()),
                exclude: Some(true),
            }),
            ..Default::default()
        }
    } else {
        ReasoningParams {
            reasoning_effort: Some(level.to_string()),
            ..Default::default()
        }
    }
}

/// DanyWhere: what is asked of the endpoint about reasoning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReasoningRequest {
    /// Without an effort, ask the endpoint to skip reasoning (Handy's
    /// `disable_reasoning`, for custom endpoints and OpenRouter).
    pub disable_by_default: bool,
    /// The effort chosen for the profile; `None` = the model's default.
    pub effort: Option<String>,
    /// DanyWhere: more fields for a chat completion request's body, from the
    /// provider's sign-in (OpenRouter's free models to fall back on, or its
    /// data policy with credit; `post_process_signin::commands`).
    pub extra_fields: serde_json::Map<String, Value>,
}

/// DanyWhere: an answer, and whether it came without the chosen effort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub content: Option<String>,
    /// An effort was chosen, but the endpoint refused it (in this request or
    /// an earlier one), so the answer came at the model's default.
    pub effort_ignored: bool,
}

/// DanyWhere: how the error of an error answer begins; its status and body
/// follow ("404 Not Found: {…}"). `post_process_failure::classify_llm_error`
/// reads them, to name the reason in the notice.
pub(crate) const STATUS_ERROR_PREFIX: &str = "API request failed with status ";

/// DanyWhere: how the error of an answer cut off at the output limit begins.
pub(crate) const TRUNCATED_ERROR_PREFIX: &str = "Response was truncated";

/// DanyWhere: what to send, in order: each body with the key under which a
/// refusal of its reasoning fields is remembered (none for the last, plain
/// one). See [`post_until_accepted`].
#[derive(Debug, PartialEq)]
struct Plan<B> {
    attempts: Vec<(B, Option<String>)>,
    /// The first body carries the chosen effort.
    tries_effort: bool,
    /// The chosen effort was refused before, so it isn't tried again.
    effort_skipped: bool,
}

impl<B> Plan<B> {
    fn plain(body: B) -> Self {
        Self {
            attempts: vec![(body, None)],
            tries_effort: false,
            effort_skipped: false,
        }
    }

    /// Whether the answer to body `used` came without the chosen effort.
    fn effort_ignored(&self, used: usize) -> bool {
        self.effort_skipped || (self.tries_effort && used > 0)
    }

    fn map<C>(self, mut body: impl FnMut(B) -> C) -> Plan<C> {
        Plan {
            attempts: self
                .attempts
                .into_iter()
                .map(|(b, key)| (body(b), key))
                .collect(),
            tries_effort: self.tries_effort,
            effort_skipped: self.effort_skipped,
        }
    }
}

/// `effort` and the plain body after it, unless the endpoint refused the
/// effort before; `fallback` when no effort is chosen.
fn with_effort<B>(
    provider: &PostProcessProvider,
    model: &str,
    effort: Option<&str>,
    effort_body: impl FnOnce(&str) -> B,
    plain: B,
    fallback: impl FnOnce(B) -> Plan<B>,
) -> Plan<B> {
    let Some(level) = effort else {
        return fallback(plain);
    };
    let key = effort_rejection_key(provider, model, level);
    if is_known_rejected(&key) {
        return Plan {
            effort_skipped: true,
            ..Plan::plain(plain)
        };
    }
    Plan {
        attempts: vec![(effort_body(level), Some(key)), (plain, None)],
        tries_effort: true,
        effort_skipped: false,
    }
}

/// The reasoning fields for an OpenAI-compatible request: the chosen effort,
/// or else Handy's reasoning-disable fields where asked for, each followed by
/// a plain request in case the endpoint refuses them.
fn reasoning_attempts(
    provider: &PostProcessProvider,
    model: &str,
    reasoning: &ReasoningRequest,
) -> Plan<ReasoningParams> {
    with_effort(
        provider,
        model,
        reasoning.effort.as_deref(),
        |level| effort_params(provider, level),
        ReasoningParams::default(),
        |plain| {
            let key = endpoint_key(provider, model);
            if reasoning.disable_by_default && !is_known_rejected(&key) {
                Plan {
                    attempts: vec![
                        (reasoning_disable_params(provider), Some(key)),
                        (plain, None),
                    ],
                    tries_effort: false,
                    effort_skipped: false,
                }
            } else {
                Plan::plain(plain)
            }
        },
    )
}

/// The Messages requests for `prompt`: with the chosen effort, then without.
fn anthropic_attempts(
    provider: &PostProcessProvider,
    model: &str,
    prompt: String,
    effort: Option<&str>,
) -> Plan<AnthropicMessagesRequest> {
    with_effort(
        provider,
        model,
        effort,
        |level| anthropic_request(provider, model, prompt.clone(), Some(level)),
        anthropic_request(provider, model, prompt.clone(), None),
        Plan::plain,
    )
}

/// Endpoints (base_url|model) that rejected the reasoning-disable fields with a
/// 4xx. Remembered for the lifetime of the process so every dictation after the
/// first skips the doomed attempt and goes straight to a plain request.
/// DanyWhere: also the efforts an endpoint refused (`effort_rejection_key`).
fn reasoning_rejections() -> &'static Mutex<HashSet<String>> {
    static REJECTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REJECTED.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(crate) fn endpoint_key(provider: &PostProcessProvider, model: &str) -> String {
    format!("{}|{}", provider.base_url.trim_end_matches('/'), model)
}

/// DanyWhere: the key under which a refused effort is remembered.
fn effort_rejection_key(provider: &PostProcessProvider, model: &str, level: &str) -> String {
    format!("{}|effort:{}", endpoint_key(provider, model), level)
}

/// DanyWhere: remembers that the endpoint refused `level` for `model`; in
/// the app, [`post_until_accepted`] does this.
#[cfg(test)]
pub(crate) fn remember_effort_rejection(provider: &PostProcessProvider, model: &str, level: &str) {
    remember_rejection(effort_rejection_key(provider, model, level));
}

/// DanyWhere: the levels the endpoint refused for `model` while the app runs.
pub(crate) fn rejected_efforts(provider: &PostProcessProvider, model: &str) -> Vec<String> {
    crate::post_process_effort::LEVELS
        .iter()
        .filter(|level| is_known_rejected(&effort_rejection_key(provider, model, level)))
        .map(|level| level.to_string())
        .collect()
}

fn is_known_rejected(key: &str) -> bool {
    reasoning_rejections()
        .lock()
        .map(|set| set.contains(key))
        .unwrap_or(false)
}

fn remember_rejection(key: String) {
    if let Ok(mut set) = reasoning_rejections().lock() {
        set.insert(key);
    }
}

#[derive(Debug, Serialize, Clone)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    #[serde(flatten)]
    reasoning: ReasoningParams,
    /// DanyWhere: [`ReasoningRequest::extra_fields`].
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
}

/// Output budget for Anthropic Messages requests, where `max_tokens` is
/// required. Ample for a cleaned-up dictation.
const ANTHROPIC_MAX_TOKENS: u32 = 4096;

/// DanyWhere: the budget with an effort, where thinking counts towards it.
/// Claude Haiku 5.5 used up to 2137 output tokens for short dictations at
/// `xhigh` (qa/2026-11-15_1450), so a long dictation at `max` could pass
/// 4096. 16000 is Anthropic's suggestion for requests without streaming, far
/// below the output limit of every model that takes an effort (64K and more);
/// only tokens used are paid for.
const ANTHROPIC_MAX_TOKENS_WITH_EFFORT: u32 = 16000;

#[derive(Debug, Serialize, Clone)]
struct AnthropicOutputConfig {
    effort: String,
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicMessagesRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ChatMessage>,
    /// DanyWhere: the effort (`post_process_effort`).
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<AnthropicOutputConfig>,
    /// DanyWhere: `{"type": "adaptive"}` with an effort, for the models that
    /// think only when asked to.
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Value>,
}

/// DanyWhere: the Messages request for `prompt`, with `effort` if one is
/// chosen.
fn anthropic_request(
    provider: &PostProcessProvider,
    model: &str,
    prompt: String,
    effort: Option<&str>,
) -> AnthropicMessagesRequest {
    let thinking = effort
        .filter(|_| crate::post_process_effort::wants_adaptive_thinking(provider, model))
        .map(|_| serde_json::json!({ "type": "adaptive" }));
    AnthropicMessagesRequest {
        model: model.to_string(),
        max_tokens: if effort.is_some() {
            ANTHROPIC_MAX_TOKENS_WITH_EFFORT
        } else {
            ANTHROPIC_MAX_TOKENS
        },
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }],
        output_config: effort.map(|effort| AnthropicOutputConfig {
            effort: effort.to_string(),
        }),
        thinking,
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicMessagesResponse {
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    /// Thinking and any other non-text blocks
    #[serde(other)]
    Other,
}

impl AnthropicMessagesResponse {
    fn into_text(self) -> Option<String> {
        let text: String = self
            .content
            .into_iter()
            .filter_map(|block| match block {
                AnthropicContentBlock::Text { text } => Some(text),
                AnthropicContentBlock::Other => None,
            })
            .collect();
        (!text.is_empty()).then_some(text)
    }
}

/// DanyWhere: the app's site, as its requests name it (`Referer`).
pub const DANYWHERE_SITE: &str = "https://github.com/yaccob/DanyWhere";
/// DanyWhere: the `User-Agent` of every request to a provider or a sign-in.
pub const DANYWHERE_USER_AGENT: &str = "DanyWhere/1.0 (+https://github.com/yaccob/DanyWhere)";

/// Build headers for API requests based on provider type
fn build_headers(provider: &PostProcessProvider, api_key: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();

    // Common headers
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(REFERER, HeaderValue::from_static(DANYWHERE_SITE));
    headers.insert(USER_AGENT, HeaderValue::from_static(DANYWHERE_USER_AGENT));
    headers.insert("X-Title", HeaderValue::from_static("DanyWhere"));

    // Provider-specific auth headers
    if !api_key.is_empty() {
        if provider.api_format == PostProcessApiFormat::Anthropic {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(api_key)
                    .map_err(|e| format!("Invalid API key header value: {}", e))?,
            );
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        } else {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", api_key))
                    .map_err(|e| format!("Invalid authorization header value: {}", e))?,
            );
        }
    }

    Ok(headers)
}

/// HTTP client with provider-specific headers. The headers are set per
/// request, so every instance can share one connection pool and consecutive
/// requests reuse an open connection instead of a new TCP + TLS handshake.
struct ProviderClient {
    client: &'static reqwest::Client,
    headers: HeaderMap,
}

impl ProviderClient {
    fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.post(url).headers(self.headers.clone())
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.get(url).headers(self.headers.clone())
    }
}

/// Process-wide client. A short idle timeout and TCP keepalive keep the pool
/// from handing out connections that died during sleep or a network change.
fn shared_client() -> Result<&'static reqwest::Client, String> {
    static SHARED: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = SHARED.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .pool_idle_timeout(Duration::from_secs(30))
        .tcp_keepalive(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| report_reqwest_error("Failed to build HTTP client", &e))?;
    Ok(SHARED.get_or_init(|| client))
}

/// Create an HTTP client with provider-specific headers
fn create_client(provider: &PostProcessProvider, api_key: &str) -> Result<ProviderClient, String> {
    Ok(ProviderClient {
        client: shared_client()?,
        headers: build_headers(provider, api_key)?,
    })
}

/// Format a bounded error source chain.
///
/// `reqwest::Error`'s Display implementation intentionally gives only a short
/// summary. Nested causes contain the useful transport details, such as a
/// certificate validation failure, an HTTP/2 error, or a connection reset.
/// Callers must skip source types whose Display text can quote payload data.
fn error_source_chain(error: &(dyn StdError + 'static)) -> Vec<String> {
    let mut causes = Vec::new();
    let mut source = error.source();

    // Defensive cap in case a third-party error exposes a cyclic source chain.
    for _ in 0..16 {
        let Some(cause) = source else {
            break;
        };
        causes.push(cause.to_string());
        source = cause.source();
    }

    causes
}

fn reqwest_error_kinds(error: &reqwest::Error) -> String {
    let mut kinds = Vec::new();

    if error.is_builder() {
        kinds.push("builder");
    }
    if error.is_connect() {
        kinds.push("connect");
    }
    if error.is_request() {
        kinds.push("request");
    }
    if error.is_redirect() {
        kinds.push("redirect");
    }
    if error.is_timeout() {
        kinds.push("timeout");
    }
    if error.is_status() {
        kinds.push("status");
    }
    if error.is_body() {
        kinds.push("body");
    }
    if error.is_decode() {
        kinds.push("decode");
    }
    if error.is_upgrade() {
        kinds.push("upgrade");
    }

    if kinds.is_empty() {
        "unknown".to_string()
    } else {
        kinds.join(", ")
    }
}

fn sanitized_url(url: &reqwest::Url) -> String {
    let mut url = url.clone();

    // Custom endpoints should not contain credentials or query-string tokens,
    // but omit them from diagnostics in case one does.
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);

    url.to_string()
}

fn sanitized_url_for_log(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|url| sanitized_url(&url))
        // Do not echo an invalid URL: the parse failure might have been caused
        // by sensitive data entered in the custom endpoint field.
        .unwrap_or_else(|_| "<invalid URL>".to_string())
}

fn report_reqwest_error(context: &str, error: &reqwest::Error) -> String {
    let kinds = reqwest_error_kinds(error);
    let url = error
        .url()
        .map(sanitized_url)
        .map(|url| format!(", url: {url}"))
        .unwrap_or_default();

    // serde_json's error text can quote values from a malformed response. That
    // response may contain transcription content, so retain the useful decode
    // classification but never put its nested source in logs or UI errors.
    let causes = if error.is_decode() {
        Vec::new()
    } else {
        error_source_chain(error)
    };
    let cause_details = if !causes.is_empty() {
        format!(": caused by: {}", causes.join(" -> "))
    } else if error.url().is_none() {
        // Reqwest's short Display text is safe when it cannot append a raw URL.
        format!(": {error}")
    } else {
        // The sanitized URL is already included above. Avoid formatting the
        // original error because its Display implementation includes the raw URL.
        String::new()
    };

    let details = format!("{context} (kind: {kinds}{url}){cause_details}");
    error!("{details}");
    details
}

/// Send a chat completion request to an OpenAI-compatible API
/// Returns Ok(Some(content)) on success, Ok(None) if response has no content,
/// or Err on actual errors (HTTP, parsing, etc.)
/// DanyWhere: the content comes in a [`Completion`], which also says whether
/// the chosen effort was refused.
pub async fn send_chat_completion(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
    reasoning: &ReasoningRequest,
) -> Result<Completion, String> {
    // DanyWhere: ChatGPT's plan, the one message as its input.
    if provider.api_format == PostProcessApiFormat::Responses {
        return send_responses(provider, model, None, &prompt, None, reasoning).await;
    }
    if provider.api_format == PostProcessApiFormat::Anthropic {
        return send_anthropic_message(
            provider,
            api_key,
            model,
            prompt,
            reasoning.effort.as_deref(),
        )
        .await;
    }

    send_chat_completion_with_schema(provider, api_key, model, prompt, None, None, reasoning).await
}

/// DanyWhere: the longest wait a rate limit's `retry-after` may ask for
/// before the one retry (sidecar note
/// `2026-11-26_2250_nachbearbeitung-einstieg-umsetzung`, section 2: Groq's
/// free plan allows 8000 tokens a minute; a longer wait becomes the notice
/// "rate-limited").
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(15);

/// DanyWhere: the wait a 429's `retry-after` asks for, in seconds (a date is
/// not read).
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    let seconds: f64 = value.parse().ok()?;
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
}

/// DanyWhere: sends the plan's bodies in turn until one is not refused with
/// 400/422, the answer endpoints give to fields they don't take. When a later
/// body gets through, the keys of the refused ones are remembered, so the
/// next request skips them. Returns the response and which body it answers.
///
/// DanyWhere (2026-11-26): a rate limit (`post_process_failure`'s
/// `rate-limited`) whose `retry-after` is at most [`MAX_RATE_LIMIT_WAIT`] is
/// waited for and sent once more; any other 429 (no credit, ChatGPT's plan
/// limit, OpenRouter's busy free model or its limits) is not.
async fn post_until_accepted<B: Serialize>(
    client: &ProviderClient,
    url: &str,
    plan: &Plan<B>,
    what: &str,
) -> Result<(reqwest::Response, usize), String> {
    let mut used = 0;
    let mut rate_limit_retried = false;
    loop {
        let Some((body, _)) = plan.attempts.get(used) else {
            return Err("No request to send".to_string());
        };
        let context = if used == 0 {
            "HTTP request failed"
        } else {
            "HTTP retry failed"
        };
        let response = client
            .post(url)
            .json(body)
            .send()
            .await
            .map_err(|e| report_reqwest_error(context, &e))?;
        let status = response.status();
        debug!(
            "{} response received with status {} over {:?} from {}",
            what,
            status,
            response.version(),
            sanitized_url(response.url())
        );

        // DanyWhere: one retry after a short rate limit.
        if status.as_u16() == 429 && !rate_limit_retried {
            if let Some(wait) =
                retry_after(response.headers()).filter(|wait| *wait <= MAX_RATE_LIMIT_WAIT)
            {
                let error_text = response.text().await.unwrap_or_else(|e| {
                    report_reqwest_error("Failed to read API error response", &e)
                });
                let error = format!("{}{}: {}", STATUS_ERROR_PREFIX, status, error_text);
                if crate::post_process_failure::classify_llm_error(&error, true)
                    != crate::post_process_failure::FailureReason::RateLimited
                {
                    return Err(error);
                }
                info!(
                    "{} was rate limited (status {}); retrying once in {:?}",
                    what, status, wait
                );
                tokio::time::sleep(wait).await;
                rate_limit_retried = true;
                continue;
            }
        }

        // A 400/422 on a request carrying reasoning fields is almost always
        // the endpoint rejecting those fields — retry without them.
        if matches!(status.as_u16(), 400 | 422) && used + 1 < plan.attempts.len() {
            let error_text = response.text().await.unwrap_or_else(|e| {
                report_reqwest_error("Failed to read reasoning rejection response", &e)
            });
            // DanyWhere: without the account's id (OpenRouter puts it there).
            let error_text = crate::post_process_failure::redact_secrets(&error_text);
            info!(
                "Endpoint rejected the request's reasoning fields (status {}): {}. Retrying without them",
                status, error_text
            );
            used += 1;
            continue;
        }

        if status.is_success() && used > 0 {
            info!(
                "Retry without the reasoning fields succeeded; '{}' will skip them from now on",
                sanitized_url_for_log(url)
            );
            for key in plan.attempts[..used]
                .iter()
                .filter_map(|(_, key)| key.clone())
            {
                remember_rejection(key);
            }
        }
        return Ok((response, used));
    }
}

/// Send a chat completion request with structured output support.
/// When json_schema is provided, uses structured outputs mode.
/// system_prompt is used as the system message when provided.
///
/// When disable_reasoning is set, the request carries the reasoning-disable
/// fields the endpoint is expected to understand. Not every OpenAI-compatible
/// endpoint accepts them (DeepSeek, Gemini's compat layer, and some OpenRouter
/// upstreams reject with 400), so a 400/422 answer to such a request triggers
/// one retry without the fields, and the rejection is remembered per
/// (base_url, model) so later requests skip the failing attempt entirely.
/// DanyWhere: `reasoning` holds Handy's `disable_reasoning` and the chosen
/// effort, which wins over it and is retried and remembered the same way.
pub async fn send_chat_completion_with_schema(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    user_content: String,
    system_prompt: Option<String>,
    json_schema: Option<Value>,
    reasoning: &ReasoningRequest,
) -> Result<Completion, String> {
    // DanyWhere: ChatGPT's plan, the system prompt as its instructions.
    if provider.api_format == PostProcessApiFormat::Responses {
        return send_responses(
            provider,
            model,
            system_prompt.as_deref(),
            &user_content,
            json_schema.as_ref(),
            reasoning,
        )
        .await;
    }
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/chat/completions", base_url);

    debug!(
        "Sending chat completion request to: {}",
        sanitized_url_for_log(&url)
    );

    let client = create_client(provider, &api_key)?;

    // Build messages vector
    let mut messages = Vec::new();

    // Add system prompt if provided
    if let Some(system) = system_prompt {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system,
        });
    }

    // Add user message
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });

    // Build response_format if schema is provided
    let response_format = json_schema.map(|schema| ResponseFormat {
        format_type: "json_schema".to_string(),
        json_schema: JsonSchema {
            name: "transcription_output".to_string(),
            strict: true,
            schema,
        },
    });

    let extra = &reasoning.extra_fields;
    let plan =
        reasoning_attempts(provider, model, reasoning).map(|reasoning| ChatCompletionRequest {
            model: model.to_string(),
            messages: messages.clone(),
            stream: false,
            response_format: response_format.clone(),
            reasoning,
            extra: extra.clone(),
        });

    let (response, used) = post_until_accepted(&client, &url, &plan, "Chat completion").await?;
    let effort_ignored = plan.effort_ignored(used);
    let status = response.status();

    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error("Failed to read API error response", &e));
        return Err(format!("{}{}: {}", STATUS_ERROR_PREFIX, status, error_text));
    }

    let completion: ChatCompletionResponse = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse API response", &e))?;

    let Some(choice) = completion.choices.into_iter().next() else {
        return Ok(Completion {
            content: None,
            effort_ignored,
        });
    };

    // A response cut off at the provider's output limit is incomplete; pasting
    // it would silently drop the end of the dictation (or, for structured
    // output, paste unparseable JSON).
    if choice.finish_reason.as_deref() == Some("length") {
        return Err(format!(
            "{} at the provider's output token limit",
            TRUNCATED_ERROR_PREFIX
        ));
    }

    Ok(Completion {
        content: choice.message.content,
        effort_ignored,
    })
}

/// DanyWhere: one post-processing request through ChatGPT's plan (the
/// provider format `Responses`, `llm_responses`), with the session's access
/// token, refreshed first when due (`post_process_signin::session`). The
/// plan route takes no `system` message, so a system prompt goes as
/// `instructions`. An effort the model refuses (400 on `reasoning.effort`)
/// is dropped, as for the other formats, and not sent again while the app
/// runs; the answer says it came without it.
async fn send_responses(
    provider: &PostProcessProvider,
    model: &str,
    instructions: Option<&str>,
    input: &str,
    json_schema: Option<&Value>,
    reasoning: &ReasoningRequest,
) -> Result<Completion, String> {
    let Some((store, token_url)) = crate::post_process_signin::session::installed() else {
        return Err(crate::post_process_signin::session::NO_SESSION.to_string());
    };
    send_responses_with(
        store.as_ref(),
        &token_url,
        provider,
        model,
        instructions,
        input,
        json_schema,
        reasoning,
    )
    .await
}

/// DanyWhere: [`send_responses`] with the session of `store`, refreshed
/// through `token_url`. Review of 2026-11-27, M2: an access token the plan
/// refuses before its expiry (signed out in ChatGPT's settings, say) is
/// renewed once and the request asked again; refused again, the sign-in
/// ends and the error reads as `sign-in-expired`.
#[allow(clippy::too_many_arguments)]
async fn send_responses_with(
    store: &dyn crate::post_process_signin::session::SessionStore,
    token_url: &str,
    provider: &PostProcessProvider,
    model: &str,
    instructions: Option<&str>,
    input: &str,
    json_schema: Option<&Value>,
    reasoning: &ReasoningRequest,
) -> Result<Completion, String> {
    use crate::post_process_signin::session;
    let token_client = crate::post_process_signin::commands::token_client()?;
    let signed_in =
        session::fresh_session(store, &token_client, token_url, session::unix_now()).await?;
    let client = shared_client()?;
    // `X-Title`, `Referer`, `User-Agent`; the bearer and the content type
    // are `llm_responses`'.
    let mut headers = build_headers(provider, "")?;
    headers.remove(CONTENT_TYPE);
    let effort = reasoning.effort.as_deref();
    let rejection = effort.map(|level| effort_rejection_key(provider, model, level));
    let skipped = rejection.as_deref().is_some_and(is_known_rejected);
    let mut attempt = PlanAttempt {
        request: crate::llm_responses::ResponsesRequest {
            model,
            instructions,
            input,
            effort: if skipped { None } else { effort },
            json_schema,
            timeout: None,
        },
        effort_ignored: skipped,
        rejection,
    };
    debug!(
        "Sending a request through ChatGPT's plan to {}",
        sanitized_url_for_log(&provider.base_url)
    );
    let error = match send_plan(client, provider, &signed_in.access_token, &headers, &mut attempt)
        .await
    {
        Err(error) if refuses_sign_in(&error) => error,
        answered => return answered,
    };
    info!(
        "ChatGPT's plan refused the sign-in (status {:?})",
        crate::post_process_failure::error_status(&error)
    );
    let renewed =
        session::renewed_after_refusal(store, &token_client, token_url, &signed_in).await?;
    match send_plan(client, provider, &renewed.access_token, &headers, &mut attempt).await {
        Err(error) if refuses_sign_in(&error) => {
            session::end_refused(store, &renewed).await;
            Err(format!(
                "{} ChatGPT's plan refused the renewed sign-in (status {})",
                crate::post_process_failure::SIGN_IN_EXPIRED_PREFIX,
                crate::post_process_failure::error_status(&error).unwrap_or_default()
            ))
        }
        answered => answered,
    }
}

/// DanyWhere: one request through ChatGPT's plan and what its effort did.
struct PlanAttempt<'a> {
    request: crate::llm_responses::ResponsesRequest<'a>,
    /// The answer came without the profile's effort.
    effort_ignored: bool,
    /// Where a refused effort is remembered (`remember_rejection`).
    rejection: Option<String>,
}

/// DanyWhere: `attempt` with the access token `bearer`. An effort the model
/// refuses (400 on `reasoning.effort`) is dropped and asked once more
/// without it, and not sent again while the app runs.
async fn send_plan(
    client: &reqwest::Client,
    provider: &PostProcessProvider,
    bearer: &str,
    headers: &HeaderMap,
    attempt: &mut PlanAttempt<'_>,
) -> Result<Completion, String> {
    let completion = |text: String, effort_ignored: bool| Completion {
        content: (!text.is_empty()).then_some(text),
        effort_ignored,
    };
    let base_url = &provider.base_url;
    match crate::llm_responses::send(client, base_url, bearer, headers, &attempt.request).await {
        Ok(text) => Ok(completion(text, attempt.effort_ignored)),
        Err(error) if attempt.request.effort.is_some() && refuses_effort(&error) => {
            info!(
                "ChatGPT's plan refused the effort for '{}'; retrying without it",
                attempt.request.model
            );
            attempt.request.effort = None;
            attempt.effort_ignored = true;
            let text =
                crate::llm_responses::send(client, base_url, bearer, headers, &attempt.request)
                    .await?;
            if let Some(key) = attempt.rejection.take() {
                remember_rejection(key);
            }
            Ok(completion(text, true))
        }
        Err(error) => Err(error),
    }
}

/// DanyWhere: whether the plan refused the sign-in itself: a 401 or 403
/// that names no other reason (not the plan's limit, a plan apps may not
/// use, a missing model or credit).
fn refuses_sign_in(error: &str) -> bool {
    use crate::post_process_failure::{classify_llm_error, error_status, FailureReason};
    matches!(error_status(error), Some(401 | 403))
        && matches!(
            classify_llm_error(error, true),
            FailureReason::KeyRefused | FailureReason::SignInExpired
        )
}

/// DanyWhere: whether `error` is a 400 refusing `reasoning.effort`.
fn refuses_effort(error: &str) -> bool {
    error
        .strip_prefix(STATUS_ERROR_PREFIX)
        .is_some_and(|rest| rest.starts_with("400") && rest.contains("reasoning.effort"))
}

/// Send a single-prompt request to an Anthropic Messages API (`/messages`).
/// Used by every provider with the Anthropic format (the built-in Anthropic
/// provider and a custom provider set to it). None of them use structured
/// output, so no reasoning-disable fields are needed.
///
/// DanyWhere: without an effort, nothing about thinking is sent, as before:
/// the Claude 5 models then think adaptively at their default effort (and
/// rarely on a short dictation), the 4.x models don't think. With an effort,
/// `output_config.effort` goes along, adaptive thinking for the models that
/// need it asked for, and a larger `max_tokens`, since thinking counts
/// towards it; an endpoint that refuses them gets the plain request again.
async fn send_anthropic_message(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
    effort: Option<&str>,
) -> Result<Completion, String> {
    let url = format!("{}/messages", provider.base_url.trim_end_matches('/'));

    debug!(
        "Sending Anthropic Messages request to: {}",
        sanitized_url_for_log(&url)
    );

    let plan = anthropic_attempts(provider, model, prompt, effort);
    let client = create_client(provider, &api_key)?;
    let (response, used) = post_until_accepted(&client, &url, &plan, "Anthropic Messages").await?;
    let effort_ignored = plan.effort_ignored(used);
    let max_tokens = plan
        .attempts
        .get(used)
        .map_or(ANTHROPIC_MAX_TOKENS, |(body, _)| body.max_tokens);
    let status = response.status();

    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error("Failed to read API error response", &e));
        return Err(format!("{}{}: {}", STATUS_ERROR_PREFIX, status, error_text));
    }

    let message: AnthropicMessagesResponse = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse API response", &e))?;

    // A response cut off at the token budget is incomplete; pasting it would
    // silently drop the end of the dictation.
    if message.stop_reason.as_deref() == Some("max_tokens") {
        return Err(format!(
            "{} at the {} token limit",
            TRUNCATED_ERROR_PREFIX, max_tokens
        ));
    }

    Ok(Completion {
        content: message.into_text(),
        effort_ignored,
    })
}

/// Fetch available models from an OpenAI-compatible API
/// Returns a list of model IDs
/// DanyWhere: `time_limit` comes from the settings (Handy: a fixed 15 s).
pub async fn fetch_models(
    provider: &PostProcessProvider,
    api_key: String,
    time_limit: Duration,
) -> Result<Vec<String>, String> {
    // DanyWhere: ChatGPT's plan lists the account's models with the
    // session's token (`chatgpt::list_models`).
    if provider.api_format == PostProcessApiFormat::Responses {
        let session = crate::post_process_signin::session::current().await?;
        let client = reqwest::Client::builder()
            .timeout(time_limit)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| report_reqwest_error("Failed to build HTTP client", &e))?;
        return crate::post_process_signin::chatgpt::list_models(
            &client,
            &provider.base_url,
            &session,
        )
        .await;
    }
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/models", base_url);

    debug!("Fetching models from: {}", sanitized_url_for_log(&url));

    let client = create_client(provider, &api_key)?;

    let response = client
        .get(&url)
        .timeout(time_limit)
        .send()
        .await
        .map_err(|e| report_reqwest_error("Failed to fetch models", &e))?;

    let status = response.status();
    debug!(
        "Model list response received with status {} over {:?} from {}",
        status,
        response.version(),
        sanitized_url(response.url())
    );
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error("Failed to read model list error", &e));
        return Err(format!(
            "Model list request failed ({}): {}",
            status, error_text
        ));
    }

    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse model list response", &e))?;

    // DanyWhere: what the list says about each model's effort.
    crate::post_process_effort::remember_listing(provider, &parsed);

    let mut models = Vec::new();

    // Handle OpenAI format: { data: [ { id: "..." }, ... ] }
    if let Some(data) = parsed.get("data").and_then(|d| d.as_array()) {
        for entry in data {
            if let Some(id) = entry.get("id").and_then(|i| i.as_str()) {
                models.push(id.to_string());
            } else if let Some(name) = entry.get("name").and_then(|n| n.as_str()) {
                models.push(name.to_string());
            }
        }
    }
    // Handle array format: [ "model1", "model2", ... ]
    else if let Some(array) = parsed.as_array() {
        for entry in array {
            if let Some(model) = entry.as_str() {
                models.push(model.to_string());
            }
        }
    }

    Ok(models)
}

/// DanyWhere: asks `provider` for a one-token answer from `model`, so that a
/// server on this computer loads the model (`model_warmup`). The answer
/// itself is thrown away; only whether the server took the request counts.
pub async fn send_warmup(
    provider: &PostProcessProvider,
    api_key: &str,
    model: &str,
    time_limit: Duration,
) -> Result<(), String> {
    // DanyWhere: through ChatGPT's plan a request would cost plan usage
    // and load nothing; only the session is made fresh.
    if provider.api_format == PostProcessApiFormat::Responses {
        return crate::post_process_signin::session::current()
            .await
            .map(|_| ());
    }
    let base_url = provider.base_url.trim_end_matches('/');
    let url = if provider.api_format == PostProcessApiFormat::Anthropic {
        format!("{}/messages", base_url)
    } else {
        format!("{}/chat/completions", base_url)
    };
    // The same body suits both formats: one short message, one token back.
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": "Hi" }],
    });
    let client = create_client(provider, api_key)?;
    let response = client
        .post(&url)
        .json(&body)
        .timeout(time_limit)
        .send()
        .await
        .map_err(|e| report_reqwest_error("Warm-up request failed", &e))?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let error_text = response
        .text()
        .await
        .unwrap_or_else(|e| report_reqwest_error("Failed to read warm-up error", &e));
    let error_text: String = error_text.chars().take(300).collect();
    Err(format!(
        "Warm-up request failed ({}): {}",
        status, error_text
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Debug)]
    struct TestError {
        message: &'static str,
        source: Option<Box<TestError>>,
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.message)
        }
    }

    impl StdError for TestError {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            self.source
                .as_deref()
                .map(|source| source as &(dyn StdError + 'static))
        }
    }

    fn provider(id: &str, base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            id: id.to_string(),
            label: id.to_string(),
            base_url: base_url.to_string(),
            allow_base_url_edit: true,
            models_endpoint: None,
            supports_structured_output: false,
            api_format: PostProcessApiFormat::OpenAi,
        }
    }

    fn anthropic_format_provider(base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            api_format: PostProcessApiFormat::Anthropic,
            ..provider("custom", base_url)
        }
    }

    fn request_json(reasoning: ReasoningParams) -> Value {
        let request = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            stream: false,
            response_format: None,
            reasoning,
            extra: serde_json::Map::new(),
        };
        serde_json::to_value(&request).unwrap()
    }

    async fn serve_one_response(status: &str, body: &str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        format!("http://{address}")
    }

    #[tokio::test]
    async fn the_model_list_request_gives_up_at_the_given_time_limit() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (_hold, release) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await;
            // Keep the connection open without a reply.
            let _ = release.await;
        });
        let provider = provider("custom", &format!("http://{address}"));

        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            // Handy's fixed 15 s would run past this.
            Duration::from_secs(8),
            fetch_models(&provider, String::new(), Duration::from_secs(1)),
        )
        .await;
        let elapsed = started.elapsed();

        assert!(
            matches!(result, Ok(Err(_))),
            "the request must fail after the 1 s it was given"
        );
        assert!(
            elapsed < Duration::from_secs(4),
            "gave up after {elapsed:?}"
        );
    }

    #[test]
    fn error_source_chain_includes_all_nested_causes() {
        let error = TestError {
            message: "request failed",
            source: Some(Box::new(TestError {
                message: "TLS handshake failed",
                source: Some(Box::new(TestError {
                    message: "unknown certificate authority",
                    source: None,
                })),
            })),
        };

        assert_eq!(
            error_source_chain(&error),
            vec!["TLS handshake failed", "unknown certificate authority"]
        );
    }

    #[test]
    fn log_url_sanitization_removes_credentials_and_tokens() {
        let url = "https://user:password@example.com/v1/models?api_key=secret#private";
        assert_eq!(sanitized_url_for_log(url), "https://example.com/v1/models");
    }

    #[test]
    fn invalid_log_urls_are_not_echoed() {
        assert_eq!(
            sanitized_url_for_log("not a URL containing secret"),
            "<invalid URL>"
        );
    }

    #[tokio::test]
    async fn decode_error_does_not_echo_response_values() {
        let base_url =
            serve_one_response("200 OK", r#"{"choices":"PRIVATE TRANSCRIPTION CONTENT"}"#).await;
        let error = reqwest::get(base_url)
            .await
            .unwrap()
            .json::<ChatCompletionResponse>()
            .await
            .unwrap_err();

        let details = report_reqwest_error("Failed to parse API response", &error);
        assert!(details.contains("kind: decode"));
        assert!(!details.contains("PRIVATE TRANSCRIPTION CONTENT"));
    }

    #[tokio::test]
    async fn raw_error_url_is_not_reintroduced_without_a_source() {
        let base_url = serve_one_response("400 Bad Request", "bad request").await;
        let error = reqwest::get(format!(
            "{base_url}/private?api_key=SECRET_QUERY_TOKEN#private"
        ))
        .await
        .unwrap()
        .error_for_status()
        .unwrap_err();

        let details = report_reqwest_error("Request failed", &error);
        assert!(details.contains(&format!("url: {base_url}/private")));
        assert!(!details.contains("SECRET_QUERY_TOKEN"));
        assert!(!details.contains("#private"));
    }

    #[test]
    fn requests_explicitly_disable_streaming() {
        let json = request_json(ReasoningParams::default());
        assert_eq!(json["stream"], false);
    }

    #[test]
    fn default_reasoning_params_serialize_to_no_fields() {
        let json = request_json(ReasoningParams::default());
        assert!(json.get("reasoning_effort").is_none());
        assert!(json.get("reasoning").is_none());
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn custom_provider_uses_top_level_reasoning_effort() {
        let params = reasoning_disable_params(&provider("custom", "http://localhost:11434/v1"));
        let json = request_json(params);
        assert_eq!(json["reasoning_effort"], "none");
        assert!(json.get("reasoning").is_none());
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn openrouter_uses_nested_reasoning_object() {
        let params =
            reasoning_disable_params(&provider("openrouter", "https://openrouter.ai/api/v1"));
        let json = request_json(params);
        assert!(json.get("reasoning_effort").is_none());
        assert_eq!(json["reasoning"]["effort"], "none");
        assert_eq!(json["reasoning"]["exclude"], true);
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn deepseek_base_url_uses_thinking_disabled() {
        let params = reasoning_disable_params(&provider("custom", "https://api.deepseek.com"));
        let json = request_json(params);
        assert!(json.get("reasoning_effort").is_none());
        assert!(json.get("reasoning").is_none());
        assert_eq!(json["thinking"]["type"], "disabled");
    }

    #[test]
    fn rejection_memo_is_keyed_by_base_url_and_model() {
        let deepseek = provider("custom", "https://api.deepseek.com/");
        let key = endpoint_key(&deepseek, "deepseek-chat");
        assert_eq!(key, "https://api.deepseek.com|deepseek-chat");
        assert!(!is_known_rejected(&key));
        remember_rejection(key.clone());
        assert!(is_known_rejected(&key));
        // A different model on the same endpoint is tracked separately
        assert!(!is_known_rejected(&endpoint_key(&deepseek, "other-model")));
    }

    #[test]
    fn anthropic_format_uses_api_key_headers() {
        let headers = build_headers(
            &anthropic_format_provider("https://example.com/anthropic/v1"),
            "key",
        )
        .unwrap();
        assert_eq!(headers["x-api-key"], "key");
        assert_eq!(headers["anthropic-version"], "2023-06-01");
        assert!(headers.get(AUTHORIZATION).is_none());

        let headers =
            build_headers(&provider("custom", "http://localhost:11434/v1"), "key").unwrap();
        assert_eq!(headers[AUTHORIZATION], "Bearer key");
        assert!(headers.get("x-api-key").is_none());
    }

    #[test]
    fn anthropic_request_carries_required_max_tokens() {
        let request = anthropic_request(
            &anthropic_format_provider("https://example.com/v1"),
            "claude-haiku",
            "hi".to_string(),
            None,
        );
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["max_tokens"], ANTHROPIC_MAX_TOKENS);
        assert_eq!(json["messages"][0]["role"], "user");
        assert!(json.get("stream").is_none());
        assert!(json.get("reasoning_effort").is_none());
    }

    #[test]
    fn anthropic_response_keeps_only_text_blocks() {
        let response: AnthropicMessagesResponse = serde_json::from_str(
            r#"{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"Hello"},{"type":"text","text":" world"}]}"#,
        )
        .unwrap();
        assert_eq!(response.into_text().as_deref(), Some("Hello world"));

        let empty: AnthropicMessagesResponse = serde_json::from_str(r#"{"content":[]}"#).unwrap();
        assert_eq!(empty.into_text(), None);
    }

    #[tokio::test]
    async fn anthropic_format_parses_messages_response() {
        let base_url = serve_one_response(
            "200 OK",
            r#"{"type":"message","role":"assistant","content":[{"type":"text","text":"Cleaned text"}]}"#,
        )
        .await;
        let result = send_chat_completion(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude-haiku",
            "raw text".to_string(),
            &ReasoningRequest {
                disable_by_default: true,
                effort: None,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(
            result,
            Ok(Completion {
                content: Some("Cleaned text".to_string()),
                effort_ignored: false,
            })
        );
    }

    #[tokio::test]
    async fn chat_completion_rejects_truncated_response() {
        let base_url = serve_one_response(
            "200 OK",
            r#"{"choices":[{"message":{"content":"Cut off"},"finish_reason":"length"}]}"#,
        )
        .await;
        let result = send_chat_completion(
            &provider("custom", &base_url),
            "key".to_string(),
            "test-model",
            "raw text".to_string(),
            &ReasoningRequest::default(),
        )
        .await;
        assert!(result.unwrap_err().contains("truncated"));
    }

    /// DanyWhere: what the post-processing notice makes of the error this
    /// client gives for an answer `status` with `body`
    /// (`post_process_failure`), sent with `api_key`.
    async fn reason_for_answer(
        status: &str,
        body: &str,
        api_key: &str,
    ) -> crate::post_process_failure::FailureReason {
        let base_url = serve_one_response(status, body).await;
        let error = send_chat_completion(
            &provider("custom", &base_url),
            api_key.to_string(),
            "gemma4-12b-qat",
            "raw text".to_string(),
            &ReasoningRequest::default(),
        )
        .await
        .unwrap_err();
        crate::post_process_failure::classify_llm_error(&error, !api_key.is_empty())
    }

    #[tokio::test]
    async fn the_errors_of_answers_name_their_reason() {
        use crate::post_process_failure::FailureReason;
        // Ollama, the model removed (user, 2026-11-23).
        assert_eq!(
            reason_for_answer(
                "404 Not Found",
                r#"{"error":{"message":"model 'gemma4-12b-qat' not found","type":"not_found_error","param":null,"code":null}}"#,
                "",
            )
            .await,
            FailureReason::ModelNotFound
        );
        // OpenAI and the providers that answer as it does.
        assert_eq!(
            reason_for_answer(
                "404 Not Found",
                r#"{"error":{"message":"The model `gemma4-12b-qat` does not exist or you do not have access to it.","type":"invalid_request_error","param":null,"code":"model_not_found"}}"#,
                "key",
            )
            .await,
            FailureReason::ModelNotFound
        );
        let refused =
            r#"{"error":{"message":"Incorrect API key provided","code":"invalid_api_key"}}"#;
        assert_eq!(
            reason_for_answer("401 Unauthorized", refused, "key").await,
            FailureReason::KeyRefused
        );
        assert_eq!(
            reason_for_answer("401 Unauthorized", refused, "").await,
            FailureReason::NoApiKey
        );
        assert_eq!(
            reason_for_answer("502 Bad Gateway", "upstream down", "key").await,
            FailureReason::Status(502)
        );
        assert_eq!(
            reason_for_answer(
                "200 OK",
                r#"{"choices":[{"message":{"content":"Cut off"},"finish_reason":"length"}]}"#,
                "key",
            )
            .await,
            FailureReason::Truncated
        );
    }

    #[tokio::test]
    async fn a_used_up_credit_is_named_from_the_answer() {
        use crate::post_process_failure::FailureReason;
        // Anthropic's own format, the answer word for word as DanyWhere's
        // log showed it on 2026-11-26.
        let base_url = serve_one_response(
            "400 Bad Request",
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits."},"request_id":"req_011ChRweqaxNgJyv6HyXKRtY"}"#,
        )
        .await;
        let error = send_chat_completion(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude-fable-5-2",
            "raw text".to_string(),
            &ReasoningRequest::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            crate::post_process_failure::classify_llm_error(&error, true),
            FailureReason::NoCredit,
            "{error}"
        );
        // OpenAI and the providers that answer as it does.
        assert_eq!(
            reason_for_answer(
                "429 Too Many Requests",
                r#"{"error":{"message":"You exceeded your current quota, please check your plan and billing details.","type":"insufficient_quota","param":null,"code":"insufficient_quota"}}"#,
                "key",
            )
            .await,
            FailureReason::NoCredit
        );
        // OpenRouter.
        assert_eq!(
            reason_for_answer(
                "402 Payment Required",
                r#"{"error":{"message":"Insufficient credits. Add more using https://openrouter.ai/settings/credits","code":402}}"#,
                "key",
            )
            .await,
            FailureReason::NoCredit
        );
    }

    #[tokio::test]
    async fn a_provider_nothing_listens_at_is_unreachable() {
        // A port that was taken and given back: nothing listens there now.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let error = send_chat_completion(
            &provider("custom", &format!("http://{address}")),
            String::new(),
            "gemma4-12b-qat",
            "raw text".to_string(),
            &ReasoningRequest::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            crate::post_process_failure::classify_llm_error(&error, false),
            crate::post_process_failure::FailureReason::Unreachable,
            "{error}"
        );
    }

    #[tokio::test]
    async fn anthropic_format_rejects_truncated_response() {
        let base_url = serve_one_response(
            "200 OK",
            r#"{"type":"message","stop_reason":"max_tokens","content":[{"type":"text","text":"Cut off"}]}"#,
        )
        .await;
        let result = send_chat_completion(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude-haiku",
            "raw text".to_string(),
            &ReasoningRequest {
                disable_by_default: true,
                effort: None,
                ..Default::default()
            },
        )
        .await;
        assert!(result.unwrap_err().contains("truncated"));
    }

    // ---- DanyWhere: the effort (`post_process_effort`) ------------------

    fn effort(level: &str) -> ReasoningRequest {
        ReasoningRequest {
            disable_by_default: false,
            effort: Some(level.to_string()),
            ..Default::default()
        }
    }

    fn first_attempt(
        provider: &PostProcessProvider,
        model: &str,
        reasoning: &ReasoningRequest,
    ) -> Value {
        request_json(
            reasoning_attempts(provider, model, reasoning).attempts[0]
                .0
                .clone(),
        )
    }

    #[test]
    fn an_effort_goes_into_the_field_the_provider_understands() {
        for (id, base_url) in [
            ("openai", "https://api.openai.com/v1"),
            ("groq", "https://api.groq.com/openai/v1"),
            ("cerebras", "https://api.cerebras.ai/v1"),
            ("zai", "https://api.z.ai/api/paas/v4"),
            (
                "bedrock_mantle",
                "https://bedrock-mantle.us-east-1.api.aws/v1",
            ),
            ("custom", "https://api.deepseek.com"),
            ("custom", "http://localhost:11434/v1"),
        ] {
            let json = first_attempt(&provider(id, base_url), "m", &effort("high"));
            assert_eq!(json["reasoning_effort"], "high", "{id} {base_url}");
            assert!(json.get("reasoning").is_none(), "{id}");
            assert!(json.get("thinking").is_none(), "{id}");
        }
        let json = first_attempt(
            &provider("openrouter", "https://openrouter.ai/api/v1"),
            "m",
            &effort("xhigh"),
        );
        assert!(json.get("reasoning_effort").is_none());
        assert_eq!(json["reasoning"]["effort"], "xhigh");
        assert_eq!(json["reasoning"]["exclude"], true);
    }

    #[test]
    fn an_effort_wins_over_switching_reasoning_off() {
        let reasoning = ReasoningRequest {
            disable_by_default: true,
            effort: Some("low".to_string()),
            ..Default::default()
        };
        let json = first_attempt(
            &provider("custom", "https://api.deepseek.com"),
            "m",
            &reasoning,
        );
        assert_eq!(json["reasoning_effort"], "low");
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn the_models_default_sends_what_was_sent_before() {
        let custom = provider("custom", "http://127.0.0.1:9/default-before");
        let plan = reasoning_attempts(
            &custom,
            "m",
            &ReasoningRequest {
                disable_by_default: true,
                effort: None,
                ..Default::default()
            },
        );
        assert_eq!(
            plan.attempts,
            vec![
                (
                    reasoning_disable_params(&custom),
                    Some(endpoint_key(&custom, "m"))
                ),
                (ReasoningParams::default(), None),
            ]
        );
        assert!(!plan.effort_ignored(1));
        let plan = reasoning_attempts(
            &provider("openai", "https://api.openai.com/v1"),
            "gpt-5",
            &ReasoningRequest::default(),
        );
        assert_eq!(plan.attempts, vec![(ReasoningParams::default(), None)]);
    }

    #[test]
    fn a_refused_effort_is_followed_by_the_plain_request() {
        let custom = provider("custom", "http://127.0.0.1:9/effort-plan");
        let plan = reasoning_attempts(&custom, "m", &effort("xhigh"));
        assert_eq!(plan.attempts.len(), 2);
        assert_eq!(plan.attempts[1], (ReasoningParams::default(), None));
        assert!(!plan.effort_ignored(0));
        assert!(plan.effort_ignored(1));
    }

    #[test]
    fn an_effort_refused_before_is_not_sent_again() {
        let custom = provider("custom", "http://127.0.0.1:9/effort-known");
        remember_effort_rejection(&custom, "m", "xhigh");
        assert_eq!(rejected_efforts(&custom, "m"), vec!["xhigh".to_string()]);
        let plan = reasoning_attempts(&custom, "m", &effort("xhigh"));
        assert_eq!(plan.attempts, vec![(ReasoningParams::default(), None)]);
        assert!(plan.effort_ignored(0));
        // Another level, or another model, is tried.
        assert!(reasoning_attempts(&custom, "m", &effort("high")).tries_effort);
        assert!(reasoning_attempts(&custom, "n", &effort("xhigh")).tries_effort);
    }

    #[test]
    fn an_anthropic_effort_goes_into_output_config_with_room_to_think() {
        let anthropic = PostProcessProvider {
            id: "anthropic".to_string(),
            ..anthropic_format_provider("https://api.anthropic.com/v1")
        };
        let json = serde_json::to_value(anthropic_request(
            &anthropic,
            "claude-sonnet-5-5",
            "hi".to_string(),
            Some("xhigh"),
        ))
        .unwrap();
        assert_eq!(json["output_config"]["effort"], "xhigh");
        assert_eq!(json["thinking"]["type"], "adaptive");
        assert_eq!(json["max_tokens"], ANTHROPIC_MAX_TOKENS_WITH_EFFORT);
        assert_eq!(json["messages"][0]["content"], "hi");

        // Opus 4.5 takes an effort, but no adaptive thinking.
        let json = serde_json::to_value(anthropic_request(
            &anthropic,
            "claude-opus-4-5",
            "hi".to_string(),
            Some("low"),
        ))
        .unwrap();
        assert_eq!(json["output_config"]["effort"], "low");
        assert!(json.get("thinking").is_none());

        // Nothing known about a proxy's model: only the effort.
        let json = serde_json::to_value(anthropic_request(
            &anthropic_format_provider("https://example.com/v1"),
            "my-proxy-model",
            "hi".to_string(),
            Some("high"),
        ))
        .unwrap();
        assert_eq!(json["output_config"]["effort"], "high");
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn the_anthropic_models_default_sends_what_was_sent_before() {
        let json = serde_json::to_value(anthropic_request(
            &anthropic_format_provider("https://api.anthropic.com/v1"),
            "claude-sonnet-5-5",
            "hi".to_string(),
            None,
        ))
        .unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["max_tokens", "messages", "model"]);
        assert_eq!(json["max_tokens"], ANTHROPIC_MAX_TOKENS);
    }

    /// Answers one request per entry of `responses`, in turn, and keeps the
    /// JSON bodies of the requests.
    async fn serve_in_turn(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (String, std::sync::Arc<Mutex<Vec<Value>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let bodies = std::sync::Arc::new(Mutex::new(Vec::new()));
        let kept = bodies.clone();
        tokio::spawn(async move {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 8192];
                loop {
                    let n = stream.read(&mut buffer).await.unwrap_or(0);
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
                            let json =
                                serde_json::from_str(&text[end + 4..]).unwrap_or(Value::Null);
                            kept.lock().unwrap().push(json);
                            break;
                        }
                    }
                }
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{address}"), bodies)
    }

    const CHAT_OK: &str =
        r#"{"choices":[{"message":{"content":"Cleaned"},"finish_reason":"stop"}]}"#;
    const MESSAGES_OK: &str = r#"{"type":"message","stop_reason":"end_turn","content":[{"type":"thinking","thinking":""},{"type":"text","text":"Cleaned"}]}"#;
    const REFUSED: &str = r#"{"error":{"message":"Unsupported value: 'reasoning_effort'"}}"#;

    #[tokio::test]
    async fn an_accepted_effort_goes_along_and_is_not_reported() {
        let (base_url, bodies) = serve_in_turn(vec![("200 OK", CHAT_OK)]).await;
        let reply = send_chat_completion(
            &provider("custom", &base_url),
            "key".to_string(),
            "model",
            "raw".to_string(),
            &effort("low"),
        )
        .await;
        assert_eq!(
            reply,
            Ok(Completion {
                content: Some("Cleaned".to_string()),
                effort_ignored: false,
            })
        );
        assert_eq!(bodies.lock().unwrap()[0]["reasoning_effort"], "low");
    }

    #[tokio::test]
    async fn a_refused_effort_is_retried_without_it_remembered_and_reported() {
        let (base_url, bodies) = serve_in_turn(vec![
            ("400 Bad Request", REFUSED),
            ("200 OK", CHAT_OK),
            ("200 OK", CHAT_OK),
        ])
        .await;
        let custom = provider("custom", &base_url);
        for _ in 0..2 {
            let reply = send_chat_completion(
                &custom,
                "key".to_string(),
                "model",
                "raw".to_string(),
                &effort("xhigh"),
            )
            .await;
            assert_eq!(
                reply,
                Ok(Completion {
                    content: Some("Cleaned".to_string()),
                    effort_ignored: true,
                })
            );
        }
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 3);
        assert_eq!(bodies[0]["reasoning_effort"], "xhigh");
        // The retry and the next dictation go without it.
        assert!(bodies[1].get("reasoning_effort").is_none(), "{}", bodies[1]);
        assert!(bodies[2].get("reasoning_effort").is_none(), "{}", bodies[2]);
        assert_eq!(
            rejected_efforts(&custom, "model"),
            vec!["xhigh".to_string()]
        );
    }

    #[tokio::test]
    async fn a_refused_anthropic_effort_is_retried_with_the_plain_request() {
        let (base_url, bodies) =
            serve_in_turn(vec![("400 Bad Request", REFUSED), ("200 OK", MESSAGES_OK)]).await;
        let reply = send_chat_completion(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude-sonnet-5-5",
            "raw".to_string(),
            &effort("max"),
        )
        .await;
        assert_eq!(
            reply,
            Ok(Completion {
                content: Some("Cleaned".to_string()),
                effort_ignored: true,
            })
        );
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies[0]["output_config"]["effort"], "max");
        assert_eq!(bodies[0]["max_tokens"], ANTHROPIC_MAX_TOKENS_WITH_EFFORT);
        assert!(bodies[1].get("output_config").is_none(), "{}", bodies[1]);
        assert!(bodies[1].get("thinking").is_none(), "{}", bodies[1]);
        assert_eq!(bodies[1]["max_tokens"], ANTHROPIC_MAX_TOKENS);
    }

    #[tokio::test]
    async fn an_anthropic_answer_with_thinking_gives_only_its_text() {
        let (base_url, bodies) = serve_in_turn(vec![("200 OK", MESSAGES_OK)]).await;
        let reply = send_chat_completion(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude-haiku-5-5",
            "raw".to_string(),
            &effort("high"),
        )
        .await;
        assert_eq!(
            reply,
            Ok(Completion {
                content: Some("Cleaned".to_string()),
                effort_ignored: false,
            })
        );
        assert_eq!(bodies.lock().unwrap()[0]["output_config"]["effort"], "high");
    }

    #[tokio::test]
    async fn a_request_failing_for_another_reason_remembers_nothing() {
        let (base_url, _bodies) = serve_in_turn(vec![
            ("400 Bad Request", REFUSED),
            ("400 Bad Request", REFUSED),
        ])
        .await;
        let custom = provider("custom", &base_url);
        let reply = send_chat_completion(
            &custom,
            "key".to_string(),
            "model",
            "raw".to_string(),
            &effort("high"),
        )
        .await;
        assert!(reply.is_err());
        assert!(rejected_efforts(&custom, "model").is_empty());
    }

    #[tokio::test]
    async fn a_warmup_asks_for_one_token_of_the_model() {
        let (base_url, bodies) = serve_in_turn(vec![("200 OK", CHAT_OK)]).await;
        let reply = send_warmup(
            &provider("custom", &base_url),
            "",
            "gemma4:12b",
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(reply, Ok(()));
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0]["model"], "gemma4:12b");
        assert_eq!(bodies[0]["max_tokens"], 1);
        assert_eq!(bodies[0]["messages"][0]["role"], "user");
    }

    #[tokio::test]
    async fn an_anthropic_format_warmup_asks_for_one_token_too() {
        let (base_url, bodies) = serve_in_turn(vec![("200 OK", MESSAGES_OK)]).await;
        let reply = send_warmup(
            &anthropic_format_provider(&base_url),
            "",
            "local-model",
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(reply, Ok(()));
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0]["model"], "local-model");
        assert_eq!(bodies[0]["max_tokens"], 1);
    }

    // ---- DanyWhere: one retry after a short rate limit (2026-11-26) --------
    //
    // Groq's free plan allows 8000 tokens a minute (sidecar note
    // `2026-11-26_1955_nachbearbeitung-versuche`, section 4) and sends
    // `retry-after` with its 429 (console.groq.com/docs/rate-limits).

    /// A status line with a `retry-after` header after it, for
    /// [`serve_in_turn`], which writes the status into its response's first
    /// line.
    const RATE_LIMITED_NOW: &str = "429 Too Many Requests\r\nretry-after: 0";
    const RATE_LIMITED_1S: &str = "429 Too Many Requests\r\nretry-after: 1";
    const RATE_LIMITED_LONG: &str = "429 Too Many Requests\r\nretry-after: 30";
    const TPM: &str = r#"{"error":{"message":"Rate limit reached for model `openai/gpt-oss-120b` in organization `org_test` service tier `on_demand` on tokens per minute (TPM): Limit 8000, Used 5959, Requested 3039. Please try again in 0.5s.","type":"tokens","code":"rate_limit_exceeded"}}"#;
    const FREE_BUSY: &str = r#"{"error":{"message":"Provider returned error","code":429,"metadata":{"raw":"google/gemma-4-26b-a4b-it:free is temporarily rate-limited upstream. Please retry shortly, or add your own provider key to use your own limits: https://openrouter.ai/workspaces/default/byok","provider_name":"Google AI Studio","is_byok":false,"provider_error_code":"429","limit_source":"upstream_provider_shared_pool"}}}"#;
    const QUOTA: &str = r#"{"error":{"message":"You exceeded your current quota, please check your plan and billing details.","type":"insufficient_quota","param":null,"code":"insufficient_quota"}}"#;

    async fn plain_request(base_url: &str) -> Result<Completion, String> {
        send_chat_completion(
            &provider("custom", base_url),
            "key".to_string(),
            "model",
            "raw".to_string(),
            &ReasoningRequest::default(),
        )
        .await
    }

    #[tokio::test]
    async fn a_short_rate_limit_is_waited_for_and_tried_once_more() {
        let (base_url, bodies) =
            serve_in_turn(vec![(RATE_LIMITED_1S, TPM), ("200 OK", CHAT_OK)]).await;
        let started = std::time::Instant::now();
        let reply = plain_request(&base_url).await;
        assert_eq!(
            reply,
            Ok(Completion {
                content: Some("Cleaned".to_string()),
                effort_ignored: false,
            })
        );
        assert!(started.elapsed() >= Duration::from_millis(900), "waited");
        assert_eq!(bodies.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_rate_limit_is_retried_only_once() {
        let (base_url, bodies) = serve_in_turn(vec![
            (RATE_LIMITED_NOW, TPM),
            (RATE_LIMITED_NOW, TPM),
            ("200 OK", CHAT_OK),
        ])
        .await;
        let reply = plain_request(&base_url).await;
        let error = reply.expect_err("the second 429 is the answer");
        assert!(error.starts_with(STATUS_ERROR_PREFIX), "{error}");
        assert!(error.contains("tokens per minute"), "{error}");
        assert_eq!(bodies.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_long_or_missing_retry_after_is_not_waited_for() {
        for status in [RATE_LIMITED_LONG, "429 Too Many Requests"] {
            let (base_url, bodies) = serve_in_turn(vec![(status, TPM), ("200 OK", CHAT_OK)]).await;
            let reply = plain_request(&base_url).await;
            assert!(reply.is_err(), "{status}");
            assert_eq!(bodies.lock().unwrap().len(), 1, "{status}");
        }
    }

    #[tokio::test]
    async fn a_busy_free_model_or_a_used_up_quota_is_not_retried() {
        // OpenRouter's busy free model sends no retry-after (strand L); even
        // with one it isn't retried, and neither is a quota that is gone.
        for body in [FREE_BUSY, QUOTA] {
            let (base_url, bodies) =
                serve_in_turn(vec![(RATE_LIMITED_NOW, body), ("200 OK", CHAT_OK)]).await;
            let reply = plain_request(&base_url).await;
            let error = reply.expect_err("not retried");
            assert!(error.starts_with(STATUS_ERROR_PREFIX), "{error}");
            assert_eq!(bodies.lock().unwrap().len(), 1, "{body}");
        }
    }

    #[tokio::test]
    async fn an_anthropic_rate_limit_is_retried_too() {
        let anthropic_429 = r#"{"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed the rate limit for your organization of 50 requests per minute."}}"#;
        let (base_url, bodies) = serve_in_turn(vec![
            (RATE_LIMITED_NOW, anthropic_429),
            ("200 OK", MESSAGES_OK),
        ])
        .await;
        let reply = send_chat_completion(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude-haiku-5-5",
            "raw".to_string(),
            &ReasoningRequest::default(),
        )
        .await;
        assert_eq!(
            reply.map(|completion| completion.content),
            Ok(Some("Cleaned".to_string()))
        );
        assert_eq!(bodies.lock().unwrap().len(), 2);
    }

    #[test]
    fn retry_after_is_read_in_seconds() {
        let header = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert("retry-after", HeaderValue::from_str(value).unwrap());
            headers
        };
        assert_eq!(retry_after(&header("8")), Some(Duration::from_secs(8)));
        assert_eq!(
            retry_after(&header("7.5")),
            Some(Duration::from_millis(7500))
        );
        assert_eq!(retry_after(&header("Wed, 21 Oct 2026 07:28:00 GMT")), None);
        assert_eq!(retry_after(&header("-1")), None);
        assert_eq!(retry_after(&HeaderMap::new()), None);
    }

    #[tokio::test]
    async fn a_refused_warmup_is_an_error() {
        let (base_url, _bodies) =
            serve_in_turn(vec![("404 Not Found", r#"{"error":"model not found"}"#)]).await;
        let reply = send_warmup(
            &provider("custom", &base_url),
            "",
            "missing",
            Duration::from_secs(5),
        )
        .await;
        assert!(reply.is_err());
    }

    // ---- DanyWhere: ChatGPT's plan over the Responses API (strand Z) -------

    const PLAN_ACCESS: &str = "test-plan-access-token";

    /// The plan session every test here sees: one that is not due, so no
    /// refresh is asked for.
    struct TestPlanStore;

    impl crate::post_process_signin::session::SessionStore for TestPlanStore {
        fn load(&self) -> Option<crate::post_process_signin::chatgpt::ChatGptSession> {
            Some(crate::post_process_signin::chatgpt::ChatGptSession {
                client_id: "app_client_test".to_string(),
                access_token: PLAN_ACCESS.to_string(),
                refresh_token: "test-plan-refresh".to_string(),
                expires_at: i64::MAX / 2,
                email: None,
                plan: None,
                subject: "user-test".to_string(),
                earliest_refresh_at: None,
            })
        }
        fn save(
            &self,
            _session: &crate::post_process_signin::chatgpt::ChatGptSession,
        ) -> Result<(), String> {
            Ok(())
        }
        fn expire(&self) {}
    }

    fn plan_session() {
        static INSTALLED: std::sync::Once = std::sync::Once::new();
        INSTALLED.call_once(|| {
            crate::post_process_signin::session::install(
                std::sync::Arc::new(TestPlanStore),
                "http://127.0.0.1:9/never".to_string(),
            )
        });
    }

    fn plan_provider(base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            api_format: PostProcessApiFormat::Responses,
            supports_structured_output: true,
            ..provider("chatgpt", base_url)
        }
    }

    /// One request as the plan's fake server saw it.
    #[derive(Debug, Clone)]
    struct PlanRequest {
        line: String,
        authorization: Option<String>,
        body: Value,
    }

    /// Answers one request per entry (status, content type, body), in turn,
    /// and keeps the requests.
    async fn serve_plan(
        replies: Vec<(&'static str, &'static str, &'static str)>,
    ) -> (String, std::sync::Arc<Mutex<Vec<PlanRequest>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        tokio::spawn(async move {
            for (status, content_type, body) in replies {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut request = Vec::new();
                let mut buffer = [0_u8; 8192];
                loop {
                    let n = stream.read(&mut buffer).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..n]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let head = &text[..end];
                        let header = |name: &str| {
                            head.lines().find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.trim()
                                    .eq_ignore_ascii_case(name)
                                    .then(|| value.trim().to_string())
                            })
                        };
                        let length = header("content-length")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            let first = head.lines().next().unwrap_or("");
                            let line = first.rsplit_once(' ').map_or(first, |(l, _)| l);
                            kept.lock().unwrap().push(PlanRequest {
                                line: line.to_string(),
                                authorization: header("authorization"),
                                body: serde_json::from_str(&text[end + 4..]).unwrap_or(Value::Null),
                            });
                            break;
                        }
                    }
                }
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        (format!("http://{address}"), seen)
    }

    const SSE: &str = "text/event-stream";
    const PLAN_OK: &str = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Clean\"}\n\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"ed\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n";
    const PLAN_EMPTY: &str =
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n";
    const PLAN_REFUSES_EFFORT: &str = r#"{"error": {"code": "unsupported_value", "message": "Unsupported value: 'xhigh' is not supported with the 'gpt-test' model.", "param": "reasoning.effort", "type": "invalid_request_error"}}"#;
    const PLAN_LIMIT: &str = r#"{"error":{"code":"subscription_sharing_usage_limit_exceeded","message":"You have reached your plan's usage limit."}}"#;

    fn plan_requests(seen: &std::sync::Arc<Mutex<Vec<PlanRequest>>>) -> Vec<PlanRequest> {
        seen.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn the_plan_gets_the_structured_request_on_responses_with_the_sessions_token() {
        plan_session();
        let (base_url, seen) = serve_plan(vec![("200 OK", SSE, PLAN_OK)]).await;
        let schema = serde_json::json!({"type": "object"});
        let reply = send_chat_completion_with_schema(
            &plan_provider(&base_url),
            String::new(),
            "gpt-6.2-sol",
            "the transcript".to_string(),
            Some("the frame".to_string()),
            Some(schema.clone()),
            &ReasoningRequest {
                effort: Some("low".to_string()),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(
            reply,
            Ok(Completion {
                content: Some("Cleaned".to_string()),
                effort_ignored: false,
            })
        );
        let requests = plan_requests(&seen);
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.line, "POST /responses");
        assert_eq!(
            request.authorization.as_deref(),
            Some(format!("Bearer {PLAN_ACCESS}").as_str())
        );
        let body = &request.body;
        assert_eq!(body["model"], "gpt-6.2-sol");
        assert_eq!(body["instructions"], "the frame");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"], "the transcript");
        assert_eq!(body["reasoning"]["effort"], "low");
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        assert_eq!(body["text"]["format"]["schema"], schema);
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert!(body.get("messages").is_none(), "{body}");
    }

    #[tokio::test]
    async fn the_plan_gets_the_single_message_as_its_input() {
        plan_session();
        let (base_url, seen) = serve_plan(vec![("200 OK", SSE, PLAN_OK)]).await;
        let reply = send_chat_completion(
            &plan_provider(&base_url),
            String::new(),
            "gpt-6.2-sol",
            "frame, prompt and transcript".to_string(),
            &ReasoningRequest::default(),
        )
        .await;
        assert_eq!(
            reply.map(|completion| completion.content),
            Ok(Some("Cleaned".to_string()))
        );
        let body = plan_requests(&seen)[0].body.clone();
        assert_eq!(body["input"][0]["content"], "frame, prompt and transcript");
        assert!(body.get("instructions").is_none(), "{body}");
        assert!(body.get("reasoning").is_none(), "{body}");
    }

    #[tokio::test]
    async fn an_empty_answer_of_the_plan_has_no_content() {
        plan_session();
        let (base_url, _seen) = serve_plan(vec![("200 OK", SSE, PLAN_EMPTY)]).await;
        let reply = send_chat_completion(
            &plan_provider(&base_url),
            String::new(),
            "gpt-6.2-sol",
            "prompt".to_string(),
            &ReasoningRequest::default(),
        )
        .await;
        assert_eq!(
            reply,
            Ok(Completion {
                content: None,
                effort_ignored: false,
            })
        );
    }

    #[tokio::test]
    async fn an_effort_the_plan_refuses_is_dropped_reported_and_not_sent_again() {
        plan_session();
        let (base_url, seen) = serve_plan(vec![
            ("400 Bad Request", "application/json", PLAN_REFUSES_EFFORT),
            ("200 OK", SSE, PLAN_OK),
            ("200 OK", SSE, PLAN_OK),
        ])
        .await;
        let provider = plan_provider(&base_url);
        let reasoning = ReasoningRequest {
            effort: Some("xhigh".to_string()),
            ..Default::default()
        };
        let ask = || {
            send_chat_completion(
                &provider,
                String::new(),
                "gpt-test-refused-effort",
                "prompt".to_string(),
                &reasoning,
            )
        };
        assert_eq!(
            ask().await,
            Ok(Completion {
                content: Some("Cleaned".to_string()),
                effort_ignored: true,
            })
        );
        assert_eq!(
            ask().await,
            Ok(Completion {
                content: Some("Cleaned".to_string()),
                effort_ignored: true,
            })
        );
        let requests = plan_requests(&seen);
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].body["reasoning"]["effort"], "xhigh");
        assert!(requests[1].body.get("reasoning").is_none());
        assert!(requests[2].body.get("reasoning").is_none(), "not again");
    }

    #[tokio::test]
    async fn a_plan_limit_keeps_the_status_form_and_its_reason() {
        plan_session();
        let (base_url, seen) = serve_plan(vec![(
            "429 Too Many Requests",
            "application/json",
            PLAN_LIMIT,
        )])
        .await;
        let error = send_chat_completion(
            &plan_provider(&base_url),
            String::new(),
            "gpt-6.2-sol",
            "prompt".to_string(),
            &ReasoningRequest::default(),
        )
        .await
        .expect_err("the limit");
        assert!(error.starts_with(STATUS_ERROR_PREFIX), "{error}");
        assert_eq!(
            crate::post_process_failure::classify_llm_error(&error, true),
            crate::post_process_failure::FailureReason::PlanLimit
        );
        assert_eq!(plan_requests(&seen).len(), 1);
    }

    #[tokio::test]
    async fn the_plans_model_list_comes_from_the_account() {
        plan_session();
        let (base_url, seen) = serve_plan(vec![(
            "200 OK",
            "application/json",
            r#"{"models":[{"slug":"gpt-6.2-sol","visibility":"list"},{"slug":"gpt-internal","visibility":"hide"},{"slug":"gpt-6.1-terra","visibility":"list"}]}"#,
        )])
        .await;
        let models = fetch_models(
            &plan_provider(&base_url),
            String::new(),
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(
            models,
            Ok(vec!["gpt-6.2-sol".to_string(), "gpt-6.1-terra".to_string()])
        );
        let request = &plan_requests(&seen)[0];
        assert_eq!(request.line, "GET /models");
        assert_eq!(
            request.authorization.as_deref(),
            Some(format!("Bearer {PLAN_ACCESS}").as_str())
        );
    }

    #[tokio::test]
    async fn warming_the_plan_up_sends_no_request() {
        plan_session();
        let (base_url, seen) = serve_plan(vec![("200 OK", SSE, PLAN_OK)]).await;
        let reply = send_warmup(
            &plan_provider(&base_url),
            "",
            "gpt-6.2-sol",
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(reply, Ok(()));
        assert!(plan_requests(&seen).is_empty());
    }

    #[tokio::test]
    async fn openrouters_fields_from_the_sign_in_go_with_the_request() {
        let (base_url, bodies) = serve_in_turn(vec![("200 OK", CHAT_OK)]).await;
        let mut extra_fields = serde_json::Map::new();
        extra_fields.insert(
            "models".to_string(),
            serde_json::json!(["a/one:free", "b/two:free"]),
        );
        let reply = send_chat_completion_with_schema(
            &provider("openrouter", &base_url),
            "sk-or-test".to_string(),
            "a/one:free",
            "content".to_string(),
            None,
            None,
            &ReasoningRequest {
                extra_fields,
                ..Default::default()
            },
        )
        .await;
        assert!(reply.is_ok(), "{reply:?}");
        let body = bodies.lock().unwrap()[0].clone();
        assert_eq!(body["model"], "a/one:free");
        assert_eq!(
            body["models"],
            serde_json::json!(["a/one:free", "b/two:free"])
        );
    }

    // ---- review of 2026-11-27, M2: a sign-in the plan refuses ------------

    /// A plan session of each test's own, and what happened to it.
    struct RefusedPlanStore {
        session: Mutex<Option<crate::post_process_signin::chatgpt::ChatGptSession>>,
        saved: std::sync::atomic::AtomicUsize,
        expired: std::sync::atomic::AtomicBool,
    }

    impl RefusedPlanStore {
        /// A session whose access token is not due for a refresh.
        fn new() -> Self {
            RefusedPlanStore {
                session: Mutex::new(Some(crate::post_process_signin::chatgpt::ChatGptSession {
                    client_id: "app_client_test".to_string(),
                    access_token: "access-old".to_string(),
                    refresh_token: "refresh-old".to_string(),
                    expires_at: i64::MAX / 2,
                    email: None,
                    plan: None,
                    subject: "user-test".to_string(),
                    earliest_refresh_at: None,
                })),
                saved: Default::default(),
                expired: Default::default(),
            }
        }

        fn was_expired(&self) -> bool {
            self.expired.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl crate::post_process_signin::session::SessionStore for RefusedPlanStore {
        fn load(&self) -> Option<crate::post_process_signin::chatgpt::ChatGptSession> {
            self.session.lock().unwrap().clone()
        }
        fn save(
            &self,
            session: &crate::post_process_signin::chatgpt::ChatGptSession,
        ) -> Result<(), String> {
            self.saved.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.session.lock().unwrap() = Some(session.clone());
            Ok(())
        }
        fn expire(&self) {
            self.expired.store(true, std::sync::atomic::Ordering::SeqCst);
            *self.session.lock().unwrap() = None;
        }
    }

    const PLAN_REFUSED_401: &str = r#"{"error":{"code":"invalid_token","message":"The access token is invalid."}}"#;
    const PLAN_NOT_ELIGIBLE_403: &str = r#"{"error":{"code":"subscription_sharing_user_not_eligible","message":"This plan can't be used by apps."}}"#;
    const RENEWED_TOKENS: &str = r#"{"access_token":"access-new","refresh_token":"refresh-new","token_type":"Bearer","expires_in":3600,"scope":"openid profile email offline_access resource.invoke chatgpt.tokens.use.direct"}"#;
    const DEAD_REFRESH: &str = r#"{"error":"invalid_grant"}"#;

    /// One request through the plan with `store`, its tokens renewed at the
    /// same fake server.
    async fn ask_plan(store: &RefusedPlanStore, base_url: &str) -> Result<Completion, String> {
        send_responses_with(
            store,
            &format!("{base_url}/api/accounts/oauth/token"),
            &plan_provider(base_url),
            "gpt-6.2-sol",
            Some("the frame"),
            "the transcript",
            None,
            &ReasoningRequest::default(),
        )
        .await
    }

    #[tokio::test]
    async fn a_refused_access_token_is_renewed_once_and_the_request_asked_again() {
        let (base_url, seen) = serve_plan(vec![
            ("401 Unauthorized", "application/json", PLAN_REFUSED_401),
            ("200 OK", "application/json", RENEWED_TOKENS),
            ("200 OK", SSE, PLAN_OK),
        ])
        .await;
        let store = RefusedPlanStore::new();
        let reply = ask_plan(&store, &base_url).await;
        assert_eq!(reply.map(|c| c.content), Ok(Some("Cleaned".to_string())));
        let requests = plan_requests(&seen);
        let lines: Vec<&str> = requests.iter().map(|r| r.line.as_str()).collect();
        assert_eq!(
            lines,
            ["POST /responses", "POST /api/accounts/oauth/token", "POST /responses"]
        );
        assert_eq!(requests[0].authorization.as_deref(), Some("Bearer access-old"));
        assert_eq!(requests[2].authorization.as_deref(), Some("Bearer access-new"));
        assert!(!store.was_expired());
        assert_eq!(
            store
                .session
                .lock()
                .unwrap()
                .as_ref()
                .map(|session| session.refresh_token.clone()),
            Some("refresh-new".to_string())
        );
    }

    #[tokio::test]
    async fn a_sign_in_refused_again_after_renewing_ends_as_expired() {
        let (base_url, seen) = serve_plan(vec![
            ("401 Unauthorized", "application/json", PLAN_REFUSED_401),
            ("200 OK", "application/json", RENEWED_TOKENS),
            ("401 Unauthorized", "application/json", PLAN_REFUSED_401),
        ])
        .await;
        let store = RefusedPlanStore::new();
        let error = ask_plan(&store, &base_url).await.expect_err("refused");
        assert!(error.starts_with("sign-in-expired:"), "{error}");
        assert_eq!(
            crate::post_process_failure::classify_llm_error(&error, false),
            crate::post_process_failure::FailureReason::SignInExpired
        );
        assert!(store.was_expired(), "the sign-in ends");
        assert_eq!(plan_requests(&seen).len(), 3);
    }

    #[tokio::test]
    async fn a_dead_refresh_token_after_a_refusal_ends_the_sign_in() {
        let (base_url, seen) = serve_plan(vec![
            ("401 Unauthorized", "application/json", PLAN_REFUSED_401),
            ("400 Bad Request", "application/json", DEAD_REFRESH),
        ])
        .await;
        let store = RefusedPlanStore::new();
        let error = ask_plan(&store, &base_url).await.expect_err("refused");
        assert_eq!(
            crate::post_process_failure::classify_llm_error(&error, false),
            crate::post_process_failure::FailureReason::SignInExpired,
            "{error}"
        );
        assert!(store.was_expired());
        assert_eq!(plan_requests(&seen).len(), 2);
    }

    #[tokio::test]
    async fn a_plan_that_may_not_be_used_is_not_renewed() {
        let (base_url, seen) = serve_plan(vec![(
            "403 Forbidden",
            "application/json",
            PLAN_NOT_ELIGIBLE_403,
        )])
        .await;
        let store = RefusedPlanStore::new();
        let error = ask_plan(&store, &base_url).await.expect_err("not eligible");
        assert_eq!(
            crate::post_process_failure::classify_llm_error(&error, false),
            crate::post_process_failure::FailureReason::PlanNotEligible
        );
        assert_eq!(plan_requests(&seen).len(), 1, "no renewal");
        assert!(!store.was_expired());
    }

    /// DanyWhere (review of 2026-11-27, N18): the requests name DanyWhere's
    /// own repository, not Handy's.
    #[test]
    fn the_requests_name_danywheres_repository() {
        let headers = build_headers(&provider("openrouter", "http://127.0.0.1:9"), "").unwrap();
        assert_eq!(
            headers.get(REFERER).and_then(|value| value.to_str().ok()),
            Some("https://github.com/yaccob/DanyWhere")
        );
        assert_eq!(
            headers.get(USER_AGENT).and_then(|value| value.to_str().ok()),
            Some("DanyWhere/1.0 (+https://github.com/yaccob/DanyWhere)")
        );
        assert_eq!(headers.get("X-Title").unwrap(), "DanyWhere");
    }
}
