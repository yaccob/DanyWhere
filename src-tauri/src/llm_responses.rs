//! DanyWhere: OpenAI's Responses API with streaming, for post-processing
//! through a ChatGPT plan ("Sign in with ChatGPT", see
//! `post_process_signin::chatgpt`).
//!
//! The plan route takes only `POST /v1/responses` with `store: false` and
//! `stream: true`, refuses a `system` message and a list of fields
//! ([`FORBIDDEN_FIELDS`]), and counts a request as done only once
//! `response.completed` arrives; a usage limit can still end it as
//! `response.failed` after text has streamed (OpenAI, "Models and inference",
//! "Preview limitations", "Errors and recovery", read 2026-11-26).
//!
//! Errors use `llm_client`'s forms, so `post_process_failure` can tell their
//! reason: an error answer as [`STATUS_ERROR_PREFIX`] with status and body, a
//! failure with a documented code inside the stream as the HTTP error that
//! code stands for, transport errors with their "(kind: …)".

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::{json, Value};
use std::error::Error as StdError;
use std::time::Duration;

/// How the error of an error answer begins (the same text as
/// `llm_client::STATUS_ERROR_PREFIX`).
pub const STATUS_ERROR_PREFIX: &str = "API request failed with status ";

/// How the error of an answer cut off at the output limit begins (the same
/// text as `llm_client::TRUNCATED_ERROR_PREFIX`).
pub const TRUNCATED_ERROR_PREFIX: &str = "Response was truncated";

/// How a failure reported inside the stream begins, when its code is none of
/// the documented ones with an HTTP status.
pub const STREAM_FAILED_PREFIX: &str = "Response failed: ";

/// The name structured output goes under (as in `llm_client`).
pub const SCHEMA_NAME: &str = "transcription_output";

/// Request fields the ChatGPT plan route refuses (OpenAI, "Preview
/// limitations"); [`request_body`] never sends them.
pub const FORBIDDEN_FIELDS: &[&str] = &[
    "background",
    "conversation",
    "max_output_tokens",
    "max_tool_calls",
    "metadata",
    "moderation",
    "multi_agent",
    "prompt",
    "prompt_cache_retention",
    "safety_identifier",
    "temperature",
    "top_logprobs",
    "top_p",
    "truncation",
    "user",
    "previous_response_id",
];

/// The User-Agent sent when `extra_headers` bring none (as `llm_client`).
const DEFAULT_USER_AGENT: &str = "DanyWhere/1.0 (+https://github.com/yaccob/Handy)";

/// One post-processing request.
pub struct ResponsesRequest<'a> {
    pub model: &'a str,
    /// Goes into `instructions` (the plan route refuses a `system` message).
    pub instructions: Option<&'a str>,
    /// The one user message.
    pub input: &'a str,
    /// `reasoning.effort`; `None` = the model's default.
    pub effort: Option<&'a str>,
    /// The schema of structured output (as `llm_client` passes it), sent as
    /// `text.format` named [`SCHEMA_NAME`], strict.
    pub json_schema: Option<&'a Value>,
    /// Limit for the whole request, the stream included; `None` = none.
    pub timeout: Option<Duration>,
}

/// The JSON body for `request`: `store: false`, `stream: true`, the input as
/// one user message, and only the options that are set. None of
/// [`FORBIDDEN_FIELDS`] ever goes along.
pub fn request_body(request: &ResponsesRequest<'_>) -> Value {
    let mut body = json!({
        "model": request.model,
        "input": [{"role": "user", "content": request.input}],
        "store": false,
        "stream": true,
    });
    if let Some(instructions) = request.instructions {
        body["instructions"] = json!(instructions);
    }
    if let Some(effort) = request.effort {
        body["reasoning"] = json!({ "effort": effort });
    }
    if let Some(schema) = request.json_schema {
        body["text"] = json!({"format": {
            "type": "json_schema",
            "name": SCHEMA_NAME,
            "schema": schema,
            "strict": true,
        }});
    }
    debug_assert!(
        FORBIDDEN_FIELDS
            .iter()
            .all(|field| body.get(field).is_none()),
        "a field the plan route refuses"
    );
    body
}

/// POST {api_base}/responses with store:false, stream:true; never role
/// "system", max_output_tokens, temperature, top_p, user. Returns the output
/// text (the JSON text with structured output; empty when the model answered
/// nothing).
///
/// `bearer` is the access token; `extra_headers` go along (the caller's
/// `X-Title`, `User-Agent`, …), `Authorization`, `Content-Type` and `Accept`
/// are set here.
pub async fn send(
    client: &reqwest::Client,
    api_base: &str,
    bearer: &str,
    extra_headers: &HeaderMap,
    request: &ResponsesRequest<'_>,
) -> Result<String, String> {
    let url = format!("{}/responses", api_base.trim_end_matches('/'));
    let body = serde_json::to_vec(&request_body(request))
        .map_err(|e| format!("Failed to encode the Responses request: {e}"))?;

    let mut headers = extra_headers.clone();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {bearer}"))
            .map_err(|_| "Invalid authorization header value".to_string())?,
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    if !headers.contains_key(USER_AGENT) {
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
    }

    log::debug!("Sending Responses request to {}", sanitized(&url));
    let mut builder = client.post(&url).headers(headers).body(body);
    if let Some(limit) = request.timeout {
        builder = builder.timeout(limit);
    }
    let response = builder
        .send()
        .await
        .map_err(|e| transport_error("Responses request failed", &e))?;

    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    if !status.is_success() {
        let text = response
            .text()
            .await
            .unwrap_or_else(|e| transport_error("Failed to read API error response", &e));
        log::warn!(
            "Responses request failed with status {status} (request id {})",
            request_id.as_deref().unwrap_or("-")
        );
        return Err(format!("{STATUS_ERROR_PREFIX}{status}: {text}"));
    }

    let kind = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    if !kind.is_empty() && kind != "text/event-stream" {
        return Err(format!(
            "Responses: expected an event stream, got content type {kind}"
        ));
    }

    let mut collector = SseCollector::new();
    let mut stream = response.bytes_stream();
    let mut outcome = Ok(());
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                if let Err(error) = collector.feed(&bytes) {
                    outcome = Err(error);
                    break;
                }
                if collector.is_completed() {
                    break;
                }
            }
            Err(error) => {
                outcome = Err(transport_error("Responses stream broke off", &error));
                break;
            }
        }
    }
    log::debug!(
        "Responses stream (request id {}): {}",
        request_id.as_deref().unwrap_or("-"),
        collector.event_types().join(", ")
    );
    outcome?;
    collector.finish()
}

/// Pure: feeds SSE bytes, collects `response.output_text.delta`, ends with
/// `response.completed`; `response.failed`/`error` events and a stream
/// without completion are errors.
///
/// Chunks may end anywhere, inside a line or a UTF-8 character; lines end
/// with LF, CRLF or CR. An event's `data:` lines are joined with LF, `event:`
/// lines are ignored (the JSON names its `type`), so are comments and
/// `[DONE]`. After `response.completed` nothing changes any more; after a
/// failure every call returns it.
#[derive(Default)]
pub struct SseCollector {
    /// The bytes of a line not ended yet.
    line: Vec<u8>,
    /// The last byte was a CR, so an LF right after it ends no second line.
    after_cr: bool,
    /// The `data:` lines of the event being read.
    data: Vec<String>,
    /// The joined deltas.
    text: String,
    /// `response.output_text.done`'s text, for a stream without deltas.
    done_text: String,
    /// The output text inside `response.completed`, likewise.
    completed_text: String,
    refusal: String,
    completed: bool,
    failure: Option<String>,
    /// Every event type seen, first appearance first.
    types: Vec<String>,
}

impl SseCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads `chunk`; the first failure in the stream comes back here.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(), String> {
        for &byte in chunk {
            if let Some(failure) = &self.failure {
                return Err(failure.clone());
            }
            if self.completed {
                return Ok(());
            }
            if std::mem::take(&mut self.after_cr) && byte == b'\n' {
                continue;
            }
            match byte {
                b'\n' | b'\r' => {
                    self.after_cr = byte == b'\r';
                    let line = std::mem::take(&mut self.line);
                    self.read_line(&line);
                }
                _ => self.line.push(byte),
            }
        }
        match &self.failure {
            Some(failure) => Err(failure.clone()),
            None => Ok(()),
        }
    }

    /// Whether `response.completed` has arrived (then the rest of the stream
    /// can be dropped).
    pub fn is_completed(&self) -> bool {
        self.completed
    }

    /// The event types seen, in the order they first came (for the log).
    pub fn event_types(&self) -> &[String] {
        &self.types
    }

    /// The output text, once the stream has ended.
    pub fn finish(mut self) -> Result<String, String> {
        // Lenient about a missing blank line at the very end.
        if !self.completed && self.failure.is_none() {
            if !self.line.is_empty() {
                let line = std::mem::take(&mut self.line);
                self.read_line(&line);
            }
            self.dispatch();
        }
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        if !self.completed {
            return Err("Responses stream ended without response.completed".to_string());
        }
        let text = [self.text, self.done_text, self.completed_text]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or_default();
        if text.is_empty() && !self.refusal.is_empty() {
            return Err(format!("The model refused: {}", self.refusal));
        }
        Ok(text)
    }

    fn read_line(&mut self, line: &[u8]) {
        if line.is_empty() {
            self.dispatch();
            return;
        }
        if line[0] == b':' {
            return;
        }
        let line = String::from_utf8_lossy(line);
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line.as_ref(), ""),
        };
        if field == "data" {
            self.data.push(value.to_string());
        }
    }

    fn dispatch(&mut self) {
        if self.data.is_empty() {
            return;
        }
        let data = std::mem::take(&mut self.data).join("\n");
        if data.trim() == "[DONE]" {
            return;
        }
        match serde_json::from_str::<Value>(&data) {
            Ok(event) => self.handle(&event),
            // serde_json's message can quote the event, which may hold the
            // transcript: name only where it broke.
            Err(error) => {
                self.failure = Some(format!(
                    "Responses stream: unreadable event (line {}, column {})",
                    error.line(),
                    error.column()
                ))
            }
        }
    }

    fn handle(&mut self, event: &Value) {
        let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
        if !self.types.iter().any(|seen| seen == kind) {
            self.types.push(kind.to_string());
        }
        let text_of = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or("");
        match kind {
            "response.output_text.delta" => self.text.push_str(text_of("delta")),
            "response.output_text.done" => self.done_text.push_str(text_of("text")),
            "response.refusal.delta" => self.refusal.push_str(text_of("delta")),
            "response.refusal.done" if self.refusal.is_empty() => {
                self.refusal.push_str(text_of("refusal"))
            }
            "response.completed" => {
                self.completed = true;
                self.completed_text = output_text(event.get("response"));
            }
            "response.failed" => {
                let error = event
                    .get("response")
                    .and_then(|response| response.get("error"))
                    .cloned()
                    .unwrap_or(Value::Null);
                self.failure = Some(failure_text(&error));
            }
            "error" => {
                let error = json!({
                    "code": event.get("code").cloned().unwrap_or(Value::Null),
                    "message": event.get("message").cloned().unwrap_or(Value::Null),
                    "param": event.get("param").cloned().unwrap_or(Value::Null),
                });
                self.failure = Some(failure_text(&error));
            }
            "response.incomplete" => {
                let reason = event
                    .pointer("/response/incomplete_details/reason")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                self.failure = Some(if reason == "max_output_tokens" {
                    format!("{TRUNCATED_ERROR_PREFIX} at the model's output limit")
                } else {
                    format!("Response incomplete: {reason}")
                });
            }
            _ => {}
        }
    }
}

/// The `output_text` parts of a finished response's message items, joined.
fn output_text(response: Option<&Value>) -> String {
    let Some(items) = response
        .and_then(|response| response.get("output"))
        .and_then(Value::as_array)
    else {
        return String::new();
    };
    items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect()
}

/// The HTTP status OpenAI documents for a Responses error code of the plan
/// route ("Errors and recovery", "Structured Responses errors").
fn documented_status(code: &str) -> Option<u16> {
    Some(match code {
        "subscription_sharing_user_not_eligible" => 403,
        "subscription_sharing_usage_limit_exceeded" => 429,
        "subscription_sharing_usage_unavailable" => 503,
        "subscription_sharing_unsupported_capability" => 400,
        "subscription_sharing_route_not_supported" => 403,
        "subscription_sharing_invalid_user" => 401,
        "chatpass_v2_scope_not_authorized" => 403,
        "chatpass_v2_invalid_authorization_context" => 403,
        "subscription_sharing_user_unavailable" => 503,
        _ => return None,
    })
}

/// A failure inside the stream: with a documented code as the HTTP error that
/// code stands for (status and `{"error": …}`, as an error answer would read),
/// otherwise [`STREAM_FAILED_PREFIX`] with code and message.
fn failure_text(error: &Value) -> String {
    let code = error.get("code").and_then(Value::as_str);
    if let Some(status) = code
        .and_then(documented_status)
        .and_then(|status| reqwest::StatusCode::from_u16(status).ok())
    {
        return format!(
            "{STATUS_ERROR_PREFIX}{status}: {}",
            json!({ "error": error })
        );
    }
    let message = error.get("message").and_then(Value::as_str).unwrap_or("");
    format!(
        "{STREAM_FAILED_PREFIX}{}: {message}",
        code.unwrap_or("unknown_error")
    )
}

/// A transport error in `llm_client::report_reqwest_error`'s form, "context
/// (kind: …, url: …): caused by: …", without query, user or password.
fn transport_error(context: &str, error: &reqwest::Error) -> String {
    let checks: [(bool, &str); 9] = [
        (error.is_builder(), "builder"),
        (error.is_connect(), "connect"),
        (error.is_request(), "request"),
        (error.is_redirect(), "redirect"),
        (error.is_timeout(), "timeout"),
        (error.is_status(), "status"),
        (error.is_body(), "body"),
        (error.is_decode(), "decode"),
        (error.is_upgrade(), "upgrade"),
    ];
    let kinds: Vec<&str> = checks
        .iter()
        .filter(|(is, _)| *is)
        .map(|(_, kind)| *kind)
        .collect();
    let kinds = if kinds.is_empty() {
        "unknown".to_string()
    } else {
        kinds.join(", ")
    };
    let url = error
        .url()
        .map(|url| format!(", url: {}", sanitized(url.as_str())))
        .unwrap_or_default();
    let mut causes = Vec::new();
    if !error.is_decode() {
        let mut source = error.source();
        while let Some(cause) = source {
            if causes.len() == 16 {
                break;
            }
            causes.push(cause.to_string());
            source = cause.source();
        }
    }
    let details = if causes.is_empty() {
        String::new()
    } else {
        format!(": caused by: {}", causes.join(" -> "))
    };
    format!("{context} (kind: {kinds}{url}){details}")
}

/// `url` without user, password, query and fragment.
fn sanitized(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut url) => {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => "<invalid URL>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // --- canned streams ------------------------------------------------------

    /// SSE as OpenAI sends it: an `event:` line naming the type, then the
    /// event as one `data:` line, then a blank line.
    fn sse(events: &[Value]) -> String {
        events
            .iter()
            .map(|event| {
                format!(
                    "event: {}\ndata: {}\n\n",
                    event["type"].as_str().unwrap_or("x"),
                    event
                )
            })
            .collect()
    }

    fn delta(text: &str) -> Value {
        json!({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 1,
               "content_index": 0, "delta": text, "sequence_number": 3, "logprobs": []})
    }

    fn created() -> Value {
        json!({"type": "response.created", "response": {"id": "resp_1", "status": "in_progress"},
               "sequence_number": 0})
    }

    fn completed() -> Value {
        json!({"type": "response.completed",
               "response": {"id": "resp_1", "status": "completed",
                            "usage": {"input_tokens": 12, "output_tokens": 3}},
               "sequence_number": 9})
    }

    fn failed(code: &str, message: &str) -> Value {
        json!({"type": "response.failed",
               "response": {"id": "resp_1", "status": "failed",
                            "error": {"code": code, "message": message}},
               "sequence_number": 5})
    }

    const TEXT: &str = "Grüße aus Zürich, 日本.";

    fn ok_stream() -> String {
        sse(&[
            created(),
            delta("Grüße "),
            delta("aus Zürich, "),
            delta("日本."),
            completed(),
        ])
    }

    fn collect(chunks: &[&[u8]]) -> Result<String, String> {
        let mut collector = SseCollector::new();
        for chunk in chunks {
            collector.feed(chunk)?;
        }
        collector.finish()
    }

    // --- the collector -------------------------------------------------------

    #[test]
    fn the_deltas_are_joined_and_completed_ends_the_answer() {
        let stream = ok_stream();
        assert_eq!(collect(&[stream.as_bytes()]), Ok(TEXT.to_string()));
    }

    #[test]
    fn every_split_of_the_bytes_gives_the_same_text() {
        let stream = ok_stream();
        let bytes = stream.as_bytes();
        for at in 0..=bytes.len() {
            let (a, b) = bytes.split_at(at);
            assert_eq!(collect(&[a, b]), Ok(TEXT.to_string()), "split at byte {at}");
        }
        let singles: Vec<&[u8]> = bytes.chunks(1).collect();
        assert_eq!(
            collect(&singles),
            Ok(TEXT.to_string()),
            "one byte at a time"
        );
    }

    #[test]
    fn crlf_and_cr_line_endings_are_lines_too() {
        let crlf = ok_stream().replace('\n', "\r\n");
        let bytes = crlf.as_bytes();
        for at in 0..=bytes.len() {
            let (a, b) = bytes.split_at(at);
            assert_eq!(collect(&[a, b]), Ok(TEXT.to_string()), "CRLF split at {at}");
        }
        let cr = ok_stream().replace('\n', "\r");
        assert_eq!(collect(&[cr.as_bytes()]), Ok(TEXT.to_string()), "CR only");
    }

    #[test]
    fn a_stream_without_completed_is_an_error() {
        let stream = sse(&[created(), delta("Halb")]);
        let error = collect(&[stream.as_bytes()]).unwrap_err();
        assert!(
            error.contains("without response.completed"),
            "error: {error}"
        );
    }

    #[test]
    fn a_documented_failure_code_reads_like_the_http_error_it_stands_for() {
        let stream = sse(&[
            created(),
            delta("x"),
            failed(
                "subscription_sharing_usage_limit_exceeded",
                "Usage limit reached.",
            ),
        ]);
        let error = collect(&[stream.as_bytes()]).unwrap_err();
        let (head, body) = error.split_once(": ").expect("status and body");
        assert_eq!(head, "API request failed with status 429 Too Many Requests");
        let body: Value = serde_json::from_str(body).expect("the body is JSON");
        assert_eq!(
            body["error"]["code"],
            "subscription_sharing_usage_limit_exceeded"
        );
        assert_eq!(body["error"]["message"], "Usage limit reached.");
    }

    #[test]
    fn every_documented_code_gets_its_status() {
        for (code, status) in [
            ("subscription_sharing_user_not_eligible", "403 Forbidden"),
            (
                "subscription_sharing_usage_limit_exceeded",
                "429 Too Many Requests",
            ),
            (
                "subscription_sharing_usage_unavailable",
                "503 Service Unavailable",
            ),
            (
                "subscription_sharing_unsupported_capability",
                "400 Bad Request",
            ),
            ("subscription_sharing_route_not_supported", "403 Forbidden"),
            ("subscription_sharing_invalid_user", "401 Unauthorized"),
            ("chatpass_v2_scope_not_authorized", "403 Forbidden"),
            ("chatpass_v2_invalid_authorization_context", "403 Forbidden"),
            (
                "subscription_sharing_user_unavailable",
                "503 Service Unavailable",
            ),
        ] {
            let stream = sse(&[failed(code, "m")]);
            let error = collect(&[stream.as_bytes()]).unwrap_err();
            assert!(
                error.starts_with(&format!("{STATUS_ERROR_PREFIX}{status}: ")),
                "{code}: {error}"
            );
        }
    }

    #[test]
    fn another_failure_code_keeps_code_and_message() {
        let stream = sse(&[failed(
            "server_error",
            "The model failed to generate a response.",
        )]);
        assert_eq!(
            collect(&[stream.as_bytes()]),
            Err(
                "Response failed: server_error: The model failed to generate a response."
                    .to_string()
            )
        );
    }

    #[test]
    fn an_error_event_is_an_error() {
        let stream = sse(&[
            created(),
            json!({"type": "error", "code": "ERR_SOMETHING", "message": "Something went wrong",
                   "param": null, "sequence_number": 1}),
        ]);
        assert_eq!(
            collect(&[stream.as_bytes()]),
            Err("Response failed: ERR_SOMETHING: Something went wrong".to_string())
        );
    }

    #[test]
    fn an_error_event_with_a_documented_code_reads_like_its_http_error() {
        let stream = sse(&[json!({"type": "error",
            "code": "subscription_sharing_unsupported_capability",
            "message": "Unsupported parameter", "param": "reasoning.effort", "sequence_number": 1})]);
        let error = collect(&[stream.as_bytes()]).unwrap_err();
        assert!(
            error.starts_with("API request failed with status 400 Bad Request: "),
            "{error}"
        );
        assert!(
            error.contains("reasoning.effort"),
            "keeps the param: {error}"
        );
    }

    #[test]
    fn incomplete_at_the_output_limit_is_a_truncated_answer() {
        let stream = sse(&[
            delta("Ein Teil"),
            json!({"type": "response.incomplete",
                   "response": {"status": "incomplete",
                                "incomplete_details": {"reason": "max_output_tokens"}}}),
        ]);
        let error = collect(&[stream.as_bytes()]).unwrap_err();
        assert!(error.starts_with(TRUNCATED_ERROR_PREFIX), "{error}");
    }

    #[test]
    fn incomplete_for_another_reason_names_it() {
        let stream = sse(&[json!({"type": "response.incomplete",
            "response": {"status": "incomplete", "incomplete_details": {"reason": "content_filter"}}})]);
        let error = collect(&[stream.as_bytes()]).unwrap_err();
        assert!(
            error.contains("incomplete") && error.contains("content_filter"),
            "{error}"
        );
        assert!(!error.starts_with(TRUNCATED_ERROR_PREFIX), "{error}");
    }

    #[test]
    fn done_comments_and_other_events_are_ignored() {
        let mut stream = String::from(": keep-alive\n\nretry: 1000\n\n");
        stream.push_str(&sse(&[
            created(),
            json!({"type": "response.in_progress", "response": {}}),
            json!({"type": "response.output_item.added", "item": {"type": "reasoning"}}),
            json!({"type": "response.reasoning_summary_text.delta", "delta": "thinking"}),
            delta("Ja."),
            json!({"type": "response.output_text.done", "text": "Ja."}),
            completed(),
        ]));
        stream.push_str("data: [DONE]\n\n");
        assert_eq!(collect(&[stream.as_bytes()]), Ok("Ja.".to_string()));
    }

    #[test]
    fn data_over_several_lines_is_one_event() {
        let stream =
            "data: {\"type\":\"response.output_text.delta\",\ndata: \"delta\":\"Zwei Zeilen\"}\n\n\
                      data:{\"type\":\"response.completed\",\"response\":{}}\n\n";
        assert_eq!(collect(&[stream.as_bytes()]), Ok("Zwei Zeilen".to_string()));
    }

    #[test]
    fn without_deltas_the_finished_text_stands_in() {
        let stream = sse(&[
            created(),
            json!({"type": "response.completed", "response": {"status": "completed", "output": [
                {"type": "reasoning", "summary": []},
                {"type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "Aus dem Ende.", "annotations": []}]}]}}),
        ]);
        assert_eq!(
            collect(&[stream.as_bytes()]),
            Ok("Aus dem Ende.".to_string())
        );
    }

    #[test]
    fn a_refusal_without_text_is_an_error() {
        let stream = sse(&[
            json!({"type": "response.refusal.delta", "delta": "I can't help with that."}),
            completed(),
        ]);
        let error = collect(&[stream.as_bytes()]).unwrap_err();
        assert!(error.contains("I can't help with that."), "{error}");
    }

    #[test]
    fn an_unreadable_event_is_an_error_that_does_not_quote_it() {
        let stream = "data: {\"type\": \"response.output_text.delta\", \"delta\": \"geheim\n\n";
        let error = collect(&[stream.as_bytes()]).unwrap_err();
        assert!(error.contains("unreadable"), "{error}");
        assert!(!error.contains("geheim"), "quotes the event: {error}");
    }

    #[test]
    fn events_after_completed_change_nothing() {
        let mut stream = ok_stream();
        stream.push_str(&sse(&[delta(" und mehr"), failed("server_error", "late")]));
        assert_eq!(collect(&[stream.as_bytes()]), Ok(TEXT.to_string()));
    }

    #[test]
    fn the_collector_says_when_it_is_done_and_which_events_came() {
        let mut collector = SseCollector::new();
        let start = sse(&[created(), delta("a")]);
        collector.feed(start.as_bytes()).unwrap();
        assert!(!collector.is_completed());
        collector.feed(sse(&[completed()]).as_bytes()).unwrap();
        assert!(collector.is_completed());
        assert_eq!(
            collector.event_types(),
            [
                "response.created",
                "response.output_text.delta",
                "response.completed"
            ]
        );
    }

    #[test]
    fn a_failure_stays_for_later_chunks() {
        let mut collector = SseCollector::new();
        let failure = sse(&[failed("server_error", "boom")]);
        assert!(collector.feed(failure.as_bytes()).is_err());
        assert!(collector.feed(ok_stream().as_bytes()).is_err());
        assert!(collector.finish().is_err());
    }

    #[test]
    fn the_prefixes_are_those_of_llm_client() {
        assert_eq!(STATUS_ERROR_PREFIX, "API request failed with status ");
        assert_eq!(TRUNCATED_ERROR_PREFIX, "Response was truncated");
    }

    // --- the request body ----------------------------------------------------

    fn schema() -> Value {
        json!({"type": "object", "properties": {"transcription": {"type": "string"}},
               "required": ["transcription"], "additionalProperties": false})
    }

    fn request<'a>(schema: Option<&'a Value>) -> ResponsesRequest<'a> {
        ResponsesRequest {
            model: "gpt-6.2-sol",
            instructions: Some("Clean up the transcript."),
            input: "<transcript>hallo welt</transcript>",
            effort: Some("low"),
            json_schema: schema,
            timeout: None,
        }
    }

    fn assert_allowed(body: &Value) {
        let object = body.as_object().expect("an object");
        for field in FORBIDDEN_FIELDS {
            assert!(!object.contains_key(*field), "sends {field}: {body}");
        }
        assert!(
            !body.to_string().contains("\"role\":\"system\""),
            "sends a system message: {body}"
        );
    }

    #[test]
    fn the_body_has_store_false_stream_true_and_the_options() {
        let schema = schema();
        let body = request_body(&request(Some(&schema)));
        assert_eq!(body["model"], "gpt-6.2-sol");
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["instructions"], "Clean up the transcript.");
        assert_eq!(
            body["input"],
            json!([{"role": "user", "content": "<transcript>hallo welt</transcript>"}])
        );
        assert_eq!(body["reasoning"], json!({"effort": "low"}));
        assert_eq!(
            body["text"],
            json!({"format": {"type": "json_schema", "name": SCHEMA_NAME,
                              "schema": schema, "strict": true}})
        );
        assert_allowed(&body);
    }

    #[test]
    fn a_plain_request_has_only_the_required_fields() {
        let body = request_body(&ResponsesRequest {
            model: "gpt-6-luna",
            instructions: None,
            input: "text",
            effort: None,
            json_schema: None,
            timeout: None,
        });
        let mut keys: Vec<&str> = body
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["input", "model", "store", "stream"]);
        assert_allowed(&body);
    }

    // --- send, against a local server ------------------------------------------

    struct Reply {
        status: &'static str,
        content_type: &'static str,
        chunks: Vec<Vec<u8>>,
        /// Keep the connection open without an answer this long.
        hold: Duration,
    }

    fn reply(status: &'static str, content_type: &'static str, chunks: Vec<String>) -> Reply {
        Reply {
            status,
            content_type,
            chunks: chunks.into_iter().map(String::into_bytes).collect(),
            hold: Duration::ZERO,
        }
    }

    /// A request as the server got it: the head (request line and headers)
    /// and the body.
    #[derive(Clone, Debug)]
    struct Seen {
        head: String,
        body: String,
    }

    impl Seen {
        fn header(&self, name: &str) -> Option<String> {
            self.head.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_string())
            })
        }
    }

    /// Answers one request per reply, in turn: the body in its chunks, a
    /// few milliseconds apart, then closes the connection (no
    /// Content-Length, so the body ends there).
    async fn serve(replies: Vec<Reply>) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        tokio::spawn(async move {
            for reply in replies {
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
                            kept.lock().unwrap().push(Seen {
                                head: text[..end].to_string(),
                                body: text[end + 4..].to_string(),
                            });
                            break;
                        }
                    }
                }
                if !reply.hold.is_zero() {
                    tokio::time::sleep(reply.hold).await;
                    continue;
                }
                let head = format!(
                    "HTTP/1.1 {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
                    reply.status, reply.content_type
                );
                let _ = stream.write_all(head.as_bytes()).await;
                for chunk in reply.chunks {
                    let _ = stream.write_all(&chunk).await;
                    let _ = stream.flush().await;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        });
        (format!("http://{address}/v1"), seen)
    }

    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("X-Title", "DanyWhere".parse().unwrap());
        headers
    }

    #[tokio::test]
    async fn send_posts_the_body_and_returns_the_streamed_text() {
        let (base, seen) = serve(vec![reply(
            "200 OK",
            "text/event-stream",
            vec![ok_stream()],
        )])
        .await;
        let schema = schema();
        let result = send(
            &reqwest::Client::new(),
            &base,
            "access-token-1",
            &headers(),
            &request(Some(&schema)),
        )
        .await;
        assert_eq!(result, Ok(TEXT.to_string()));
        let seen = seen.lock().unwrap()[0].clone();
        assert!(
            seen.head.starts_with("POST /v1/responses HTTP/1.1"),
            "{}",
            seen.head
        );
        assert_eq!(
            seen.header("authorization").as_deref(),
            Some("Bearer access-token-1")
        );
        assert_eq!(seen.header("accept").as_deref(), Some("text/event-stream"));
        assert_eq!(
            seen.header("content-type").as_deref(),
            Some("application/json")
        );
        assert_eq!(seen.header("x-title").as_deref(), Some("DanyWhere"));
        assert!(
            seen.header("user-agent")
                .is_some_and(|agent| agent.starts_with("DanyWhere/")),
            "user agent: {:?}",
            seen.header("user-agent")
        );
        let body: Value = serde_json::from_str(&seen.body).expect("a JSON body");
        assert_eq!(body, request_body(&request(Some(&schema))));
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["stream"], json!(true));
        assert_allowed(&body);
    }

    #[tokio::test]
    async fn an_answer_in_many_small_pieces_arrives_whole() {
        // Seven bytes each, so pieces also end inside a multibyte character.
        let pieces = ok_stream()
            .as_bytes()
            .chunks(7)
            .map(<[u8]>::to_vec)
            .collect();
        let (base, _) = serve(vec![Reply {
            status: "200 OK",
            content_type: "text/event-stream",
            chunks: pieces,
            hold: Duration::ZERO,
        }])
        .await;
        let result = send(
            &reqwest::Client::new(),
            &base,
            "t",
            &HeaderMap::new(),
            &request(None),
        )
        .await;
        assert_eq!(result, Ok(TEXT.to_string()));
    }

    #[tokio::test]
    async fn http_429_keeps_status_and_body() {
        let body = r#"{"error":{"code":"subscription_sharing_usage_limit_exceeded","message":"You have reached your usage limit.","param":null}}"#;
        let (base, _) = serve(vec![reply(
            "429 Too Many Requests",
            "application/json",
            vec![body.to_string()],
        )])
        .await;
        let result = send(
            &reqwest::Client::new(),
            &base,
            "t",
            &HeaderMap::new(),
            &request(None),
        )
        .await;
        assert_eq!(
            result,
            Err(format!(
                "API request failed with status 429 Too Many Requests: {body}"
            ))
        );
    }

    #[tokio::test]
    async fn http_403_with_detail_keeps_status_and_body() {
        let body = r#"{"detail":"Unsupported region"}"#;
        let (base, _) = serve(vec![reply(
            "403 Forbidden",
            "application/json",
            vec![body.to_string()],
        )])
        .await;
        let result = send(
            &reqwest::Client::new(),
            &base,
            "t",
            &HeaderMap::new(),
            &request(None),
        )
        .await;
        assert_eq!(
            result,
            Err(format!(
                "API request failed with status 403 Forbidden: {body}"
            ))
        );
    }

    #[tokio::test]
    async fn a_failure_inside_the_stream_comes_back() {
        let stream = sse(&[
            created(),
            delta("x"),
            failed("subscription_sharing_usage_limit_exceeded", "limit"),
        ]);
        let (base, _) = serve(vec![reply("200 OK", "text/event-stream", vec![stream])]).await;
        let error = send(
            &reqwest::Client::new(),
            &base,
            "t",
            &HeaderMap::new(),
            &request(None),
        )
        .await
        .unwrap_err();
        assert!(
            error.starts_with("API request failed with status 429 Too Many Requests: "),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_cut_off_stream_is_an_error() {
        let stream = sse(&[created(), delta("Halb")]);
        let (base, _) = serve(vec![reply("200 OK", "text/event-stream", vec![stream])]).await;
        let error = send(
            &reqwest::Client::new(),
            &base,
            "t",
            &HeaderMap::new(),
            &request(None),
        )
        .await
        .unwrap_err();
        assert!(error.contains("without response.completed"), "{error}");
    }

    #[tokio::test]
    async fn an_answer_that_is_no_event_stream_is_an_error() {
        let (base, _) = serve(vec![reply(
            "200 OK",
            "application/json",
            vec![r#"{"output_text":"x"}"#.to_string()],
        )])
        .await;
        let error = send(
            &reqwest::Client::new(),
            &base,
            "t",
            &HeaderMap::new(),
            &request(None),
        )
        .await
        .unwrap_err();
        assert!(
            error.contains("event stream") && error.contains("application/json"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn the_time_limit_ends_a_silent_request_as_a_timeout() {
        let (base, _) = serve(vec![Reply {
            status: "200 OK",
            content_type: "text/event-stream",
            chunks: Vec::new(),
            hold: Duration::from_secs(10),
        }])
        .await;
        let mut timed = request(None);
        timed.timeout = Some(Duration::from_millis(500));
        let started = std::time::Instant::now();
        let error = send(
            &reqwest::Client::new(),
            &base,
            "t",
            &HeaderMap::new(),
            &timed,
        )
        .await
        .unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "took {:?}",
            started.elapsed()
        );
        // The form `post_process_failure::transport_kinds` reads.
        assert!(
            error.contains("(kind: ") && error.contains("timeout"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_connect_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let error = send(
            &reqwest::Client::new(),
            &format!("http://{address}/v1"),
            "t",
            &HeaderMap::new(),
            &request(None),
        )
        .await
        .unwrap_err();
        assert!(
            error.contains("(kind: ") && error.contains("connect"),
            "{error}"
        );
        assert!(!error.contains("Bearer"), "{error}");
    }
}
