//! DanyWhere: "Sign in with ChatGPT" with plan usage, as an open-source app
//! running on the user's computer does it (OpenAI, "Registration and
//! sign-in", "Accounts and sessions", "Models and inference", "Token
//! reference", "Errors and recovery", read 2026-11-26).
//!
//! - The first sign-in registers DanyWhere for the account: `client_id` is
//!   `dynamic_agent_client` with `agent_name_hint`, and the callback hands
//!   back the issued client id ([`SignInCode::client_id`]), which every later
//!   sign-in and refresh uses. `ext_agent_host_id` stays the same per
//!   installation ([`new_host_id`] once).
//! - The callback goes to `http://127.0.0.1:<port>/auth/callback`, never
//!   `localhost`; only the port may change between sign-ins.
//! - The ID token's claims are checked (issuer, audience, expiry, nonce),
//!   not its signature: it comes straight from the token endpoint over TLS
//!   (OpenID Connect Core 1.0, 3.1.3.7, point 6).
//! - Plan usage needs `chatgpt.tokens.use.direct` among the granted scopes.
//! - Access tokens live an hour, refresh tokens 30 days, and every refresh
//!   replaces the refresh token: the caller must store the new session at
//!   once and never refresh one session twice at the same time.
//!
//! Errors of the token endpoint and the model list keep `llm_client`'s form,
//! "API request failed with status {status}: {body}".

use super::loopback::{CallbackHost, CallbackSpec, Pkce};
use super::{SignInCode, SignInFlow};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use reqwest::header::{HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::Value;
use std::error::Error as StdError;

pub const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
pub const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
/// The issuer the ID token must name.
pub const ISSUER: &str = "https://auth.openai.com";
/// OpenAI's discovery document; its `revocation_endpoint` ends a renewable
/// session ("Accounts and sessions", "End the renewable session"; on
/// 2026-11-27 it named `https://auth.openai.com/api/accounts/oauth/revoke`).
pub const DISCOVERY: &str = "https://auth.openai.com/.well-known/openid-configuration";
/// The `resource` of the authorization, the exchange and every refresh.
pub const RESOURCE: &str = "https://api.openai.com/v1";
/// Where the model list and the Responses API live.
pub const API_BASE: &str = "https://api.openai.com/v1";
/// Identity scopes, then those of plan usage.
pub const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The scope without which the plan may not be used.
pub const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
/// The client id of a first registration; never stored, never exchanged.
pub const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
pub const CALLBACK_PATH: &str = "/auth/callback";
/// The app's name as `agent_name_hint` (the user may change it in the browser).
pub const AGENT_NAME: &str = "DanyWhere";
pub const DEFAULT_MODEL: &str = "gpt-6.2-sol";
pub const DEFAULT_EFFORT: &str = "low";
/// Where the user sees and limits what DanyWhere uses of the plan. The
/// frontend links there itself ("Manage usage"); kept here beside the other
/// addresses of the plan.
#[allow(dead_code)]
pub const USAGE_URL: &str = "https://chatgpt.com/settings/usage";
/// A refresh is due when less than this is left of the access token.
pub const REFRESH_MARGIN_SECS: i64 = 300;

const USER_AGENT_VALUE: &str = crate::llm_client::DANYWHERE_USER_AGENT;
const FORM: &str = "application/x-www-form-urlencoded";
/// Clock difference allowed for the ID token's expiry.
const ID_TOKEN_LEEWAY_SECS: i64 = 60;
/// Where the ID token may name the plan (not in the trial's token).
const AUTH_CLAIM: &str = "https://api.openai.com/auth";
/// Refresh errors after which only a new sign-in helps ("Errors and
/// recovery", "Refresh errors"). `invalid_client` too: a dynamically
/// registered client can only be registered anew.
const REFRESH_DEAD: &[&str] = &[
    "invalid_grant",
    "invalid_refresh_token",
    "token_expired",
    "refresh_token_expired",
    "refresh_token_invalidated",
    "refresh_token_reused",
    "invalid_client",
];

/// One sign-in attempt. Make a new one (fresh nonce) for every attempt.
pub struct ChatGptFlow {
    /// The issued client id of an earlier registration; `None`: first
    /// registration with `dynamic_agent_client`.
    pub client_id: Option<String>,
    /// `urn:uuid:…`, stable per installation ([`new_host_id`]).
    pub host_id: String,
    /// `agent_name_hint` of a first registration ("DanyWhere").
    pub agent_name: String,
    /// The OpenID Connect nonce of this attempt; the ID token must carry it.
    pub nonce: String,
}

impl ChatGptFlow {
    /// An attempt with a fresh nonce and DanyWhere's name.
    pub fn new(client_id: Option<String>, host_id: String) -> Result<Self, String> {
        Ok(Self {
            client_id,
            host_id,
            agent_name: AGENT_NAME.to_string(),
            nonce: random_value()?,
        })
    }

    /// The saved issued client id, if there is a usable one.
    fn saved_client(&self) -> Option<&str> {
        self.client_id
            .as_deref()
            .filter(|id| !id.is_empty() && *id != DYNAMIC_CLIENT)
    }
}

impl SignInFlow for ChatGptFlow {
    fn callback(&self) -> CallbackSpec {
        CallbackSpec {
            host: CallbackHost::Ip127,
            path: CALLBACK_PATH,
        }
    }

    /// No `id_token_hint` and no `login_hint`: the URL is shown again
    /// ("open the page again") and may reach a log; the account selector
    /// costs one click.
    fn authorize_url(&self, redirect_uri: &str, state: &str, pkce: &Pkce) -> String {
        let saved = self.saved_client();
        let mut params: Vec<(&str, &str)> = vec![
            ("client_id", saved.unwrap_or(DYNAMIC_CLIENT)),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri),
            ("scope", SCOPES),
            ("resource", RESOURCE),
            ("state", state),
            ("nonce", &self.nonce),
            ("code_challenge_method", "S256"),
            ("code_challenge", &pkce.challenge),
            ("ext_agent_host_id", &self.host_id),
        ];
        if saved.is_none() {
            params.push(("agent_name_hint", &self.agent_name));
        }
        format!("{AUTHORIZE}?{}", encode_pairs(&params))
    }
}

/// A signed-in ChatGPT account. Stored as it is; Debug hides tokens, email
/// and subject.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatGptSession {
    /// The issued client id (never `dynamic_agent_client`).
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: String,
    /// When the access token ends, Unix seconds.
    pub expires_at: i64,
    pub email: Option<String>,
    /// `chatgpt_plan_type` of the ID token, when it names one.
    pub plan: Option<String>,
    /// The ID token's `sub`: a refresh must not switch the account.
    #[serde(default)]
    pub subject: String,
    /// The token endpoint's `earliest_refresh_at`, Unix seconds.
    #[serde(default)]
    pub earliest_refresh_at: Option<i64>,
}

impl std::fmt::Debug for ChatGptSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const HIDDEN: &str = "[redacted]";
        f.debug_struct("ChatGptSession")
            .field("client_id", &self.client_id)
            .field("access_token", &HIDDEN)
            .field("refresh_token", &HIDDEN)
            .field("expires_at", &self.expires_at)
            .field("email", &self.email.as_ref().map(|_| HIDDEN))
            .field("plan", &self.plan)
            .field("subject", &HIDDEN)
            .field("earliest_refresh_at", &self.earliest_refresh_at)
            .finish()
    }
}

/// Why a sign-in or refresh did not give a usable session.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatGptError {
    /// The refresh token is dead (its code): sign in again, with the saved
    /// client id (after `invalid_client` with a new registration).
    SignInExpired(String),
    /// Signed in, but plan usage was not granted (`chatgpt.tokens.use.direct`
    /// missing).
    NotEligible,
    /// The answer of the sign-in can't be trusted or is incomplete (ID token,
    /// client id, fields).
    Invalid(String),
    /// An error answer ("API request failed with status …") or a transport
    /// error; the session stays as it was.
    Http(String),
}

impl std::fmt::Display for ChatGptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignInExpired(code) => write!(
                f,
                "sign-in-expired: the ChatGPT sign-in has to be renewed ({code})"
            ),
            Self::NotEligible => write!(
                f,
                "plan-not-eligible: the sign-in does not allow using the ChatGPT plan ({PLAN_SCOPE} not granted)"
            ),
            Self::Invalid(detail) => write!(f, "ChatGPT sign-in not accepted: {detail}"),
            Self::Http(text) => f.write_str(text),
        }
    }
}

impl StdError for ChatGptError {}

/// A new `ext_agent_host_id`: `urn:uuid:` with a random version 4 UUID.
/// Make it once per installation and keep it.
pub fn new_host_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("No random bytes for the host id: {e}"))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// Exchanges the callback's code for a session. The issued client id is the
/// callback's (a first registration must bring one, a new registration for
/// a dead client brings a new one), else the flow's saved one.
pub async fn exchange_code(
    client: &reqwest::Client,
    token_url: &str,
    flow: &ChatGptFlow,
    code: &SignInCode,
) -> Result<ChatGptSession, ChatGptError> {
    let issued = issued_client(flow, code)?;
    let (status, text) = post_form(
        client,
        token_url,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", issued),
            ("code", &code.code),
            ("code_verifier", &code.verifier),
            ("redirect_uri", &code.redirect_uri),
            ("resource", RESOURCE),
        ],
    )
    .await?;
    if !status.is_success() {
        return Err(ChatGptError::Http(status_error(status, &text)));
    }
    let now = unix_now();
    let tokens = TokenAnswer::read(&text)?;
    let refresh_token = tokens.refresh_token.ok_or_else(|| {
        ChatGptError::Invalid("no refresh token although offline_access was asked for".into())
    })?;
    let id_token = tokens
        .id_token
        .ok_or_else(|| ChatGptError::Invalid("no ID token".into()))?;
    let claims = check_id_token(&id_token, issued, Some(&flow.nonce), now)?;
    let scope = tokens
        .scope
        .ok_or_else(|| ChatGptError::Invalid("the answer names no granted scopes".into()))?;
    if !has_plan_scope(&scope) {
        return Err(ChatGptError::NotEligible);
    }
    Ok(ChatGptSession {
        client_id: issued.to_string(),
        access_token: tokens.access_token,
        refresh_token,
        expires_at: now + tokens.expires_in,
        email: claims.email,
        plan: claims.plan,
        subject: claims.subject,
        earliest_refresh_at: tokens.earliest_refresh_at,
    })
}

/// A new session for `session` (new access token and the replacing refresh
/// token; store it at once). Without `scope` the grant stays as it was.
pub async fn refresh(
    client: &reqwest::Client,
    token_url: &str,
    session: &ChatGptSession,
) -> Result<ChatGptSession, ChatGptError> {
    let (status, text) = post_form(
        client,
        token_url,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &session.client_id),
            ("refresh_token", &session.refresh_token),
            ("resource", RESOURCE),
        ],
    )
    .await?;
    if !status.is_success() {
        if let Some(code) = error_code(&text).filter(|code| REFRESH_DEAD.contains(&code.as_str())) {
            return Err(ChatGptError::SignInExpired(code));
        }
        return Err(ChatGptError::Http(status_error(status, &text)));
    }
    let now = unix_now();
    let tokens = TokenAnswer::read(&text)?;
    if tokens
        .scope
        .as_deref()
        .is_some_and(|scope| !has_plan_scope(scope))
    {
        return Err(ChatGptError::NotEligible);
    }
    let mut renewed = session.clone();
    if let Some(id_token) = &tokens.id_token {
        let claims = check_id_token(id_token, &session.client_id, None, now)?;
        if !session.subject.is_empty() && claims.subject != session.subject {
            return Err(ChatGptError::Invalid(
                "the renewed ID token belongs to another account".into(),
            ));
        }
        renewed.subject = claims.subject;
        renewed.email = claims.email.or(renewed.email);
        renewed.plan = claims.plan.or(renewed.plan);
    }
    renewed.access_token = tokens.access_token;
    if let Some(refresh_token) = tokens.refresh_token {
        renewed.refresh_token = refresh_token;
    }
    renewed.expires_at = now + tokens.expires_in;
    renewed.earliest_refresh_at = tokens.earliest_refresh_at;
    Ok(renewed)
}

/// Ends `session`'s renewable session at OpenAI when signing out
/// ("Accounts and sessions", "End the renewable session", read 2026-11-27):
/// a form-encoded `POST` with `token=<refresh token>`,
/// `token_type_hint=refresh_token` and the issued `client_id` to the
/// `revocation_endpoint` of the discovery document at `discovery_url`. An
/// empty 200 is success, also for a token already invalid. The endpoint must
/// be on the discovery document's origin, so the refresh token goes nowhere
/// else; use a client without redirects. Errors keep `llm_client`'s form.
pub async fn revoke(
    client: &reqwest::Client,
    discovery_url: &str,
    session: &ChatGptSession,
) -> Result<(), String> {
    let endpoint = revocation_endpoint(client, discovery_url).await?;
    let (status, text) = post_form(
        client,
        &endpoint,
        &[
            ("token", &session.refresh_token),
            ("token_type_hint", "refresh_token"),
            ("client_id", &session.client_id),
        ],
    )
    .await
    .map_err(|error| error.to_string())?;
    if status.is_success() {
        Ok(())
    } else {
        Err(status_error(status, &text))
    }
}

/// The `revocation_endpoint` of the discovery document at `discovery_url`,
/// if it is on the same origin.
async fn revocation_endpoint(
    client: &reqwest::Client,
    discovery_url: &str,
) -> Result<String, String> {
    let response = client
        .get(discovery_url)
        .header(ACCEPT, "application/json")
        .header(USER_AGENT, USER_AGENT_VALUE)
        .send()
        .await
        .map_err(|e| transport_error("OpenAI's discovery document request failed", &e))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| transport_error("Failed to read OpenAI's discovery document", &e))?;
    if !status.is_success() {
        return Err(status_error(status, &text));
    }
    let document: Value = serde_json::from_str(&text)
        .map_err(|_| "OpenAI's discovery document is no JSON".to_string())?;
    let endpoint = document
        .get("revocation_endpoint")
        .and_then(Value::as_str)
        .filter(|endpoint| !endpoint.is_empty())
        .ok_or_else(|| "OpenAI's discovery document names no revocation_endpoint".to_string())?;
    let same_origin = match (
        reqwest::Url::parse(discovery_url),
        reqwest::Url::parse(endpoint),
    ) {
        (Ok(discovery), Ok(target)) => discovery.origin() == target.origin(),
        _ => false,
    };
    if !same_origin {
        return Err(format!(
            "The revocation endpoint {endpoint} is not on the discovery document's origin"
        ));
    }
    Ok(endpoint.to_string())
}

/// Refresh when less than five minutes are left; while the token still
/// works, not before the endpoint's `earliest_refresh_at`.
pub fn needs_refresh(session: &ChatGptSession, now_unix: i64) -> bool {
    let left = session.expires_at - now_unix;
    if left <= 0 {
        return true;
    }
    if left >= REFRESH_MARGIN_SECS {
        return false;
    }
    session
        .earliest_refresh_at
        .is_none_or(|earliest| now_unix >= earliest)
}

/// The account's models for a picker: the slugs with visibility "list", in
/// the server's order (`GET {api_base}/models`).
pub async fn list_models(
    client: &reqwest::Client,
    api_base: &str,
    session: &ChatGptSession,
) -> Result<Vec<String>, String> {
    let url = format!("{}/models", api_base.trim_end_matches('/'));
    let bearer = HeaderValue::from_str(&format!("Bearer {}", session.access_token))
        .map_err(|_| "Invalid authorization header value".to_string())?;
    let response = client
        .get(&url)
        .header(AUTHORIZATION, bearer)
        .header(ACCEPT, "application/json")
        .header(USER_AGENT, USER_AGENT_VALUE)
        .send()
        .await
        .map_err(|e| transport_error("ChatGPT model list request failed", &e))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| transport_error("Failed to read the ChatGPT model list", &e))?;
    if !status.is_success() {
        return Err(status_error(status, &text));
    }
    let catalog: Value =
        serde_json::from_str(&text).map_err(|_| "The ChatGPT model list is no JSON".to_string())?;
    let models = catalog
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| "The ChatGPT model list has no `models`".to_string())?;
    Ok(models
        .iter()
        .filter(|model| model.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|model| model.get("slug").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}

// --- helpers -------------------------------------------------------------------

/// Whether `error` says the client itself is gone (`invalid_client`, from a
/// refresh or the code's exchange), so the saved client id must go and the
/// next sign-in registers anew (review of 2026-11-27, M3).
pub fn names_dead_client(error: &ChatGptError) -> bool {
    const DEAD_CLIENT: &str = "invalid_client";
    match error {
        ChatGptError::SignInExpired(code) => code == DEAD_CLIENT,
        ChatGptError::Http(text) => text
            .strip_prefix(crate::llm_client::STATUS_ERROR_PREFIX)
            .and_then(|answer| answer.split_once(": "))
            .and_then(|(_, body)| error_code(body))
            .is_some_and(|code| code == DEAD_CLIENT),
        _ => false,
    }
}

/// The client id to exchange the code with ("Registration and sign-in",
/// step 3). One the callback names wins over the saved one: OpenAI issued
/// it anew for a saved client it no longer knows (review of 2026-11-27,
/// M3); the ID token must name it as its audience all the same.
fn issued_client<'a>(flow: &'a ChatGptFlow, code: &'a SignInCode) -> Result<&'a str, ChatGptError> {
    match (flow.saved_client(), code.client_id.as_deref()) {
        (_, Some(id)) if id.is_empty() || id == DYNAMIC_CLIENT => Err(ChatGptError::Invalid(
            "the callback named no issued client id".into(),
        )),
        (_, Some(id)) => Ok(id),
        (Some(saved), None) => Ok(saved),
        (None, None) => Err(ChatGptError::Invalid(
            "registration incomplete: the callback named no issued client id".into(),
        )),
    }
}

/// The fields of a token answer DanyWhere uses.
struct TokenAnswer {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    scope: Option<String>,
    expires_in: i64,
    earliest_refresh_at: Option<i64>,
}

impl TokenAnswer {
    fn read(text: &str) -> Result<Self, ChatGptError> {
        let data: Value = serde_json::from_str(text)
            .map_err(|_| ChatGptError::Invalid("the token answer is no JSON".into()))?;
        let string = |key: &str| {
            data.get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };
        let access_token = string("access_token")
            .ok_or_else(|| ChatGptError::Invalid("no access token".into()))?;
        if !string("token_type").is_some_and(|kind| kind.eq_ignore_ascii_case("bearer")) {
            return Err(ChatGptError::Invalid("the token is no bearer token".into()));
        }
        let expires_in = data
            .get("expires_in")
            .and_then(seconds)
            .filter(|secs| *secs > 0)
            .ok_or_else(|| ChatGptError::Invalid("no valid expires_in".into()))?;
        Ok(Self {
            access_token,
            refresh_token: string("refresh_token"),
            id_token: string("id_token"),
            scope: data
                .get("scope")
                .and_then(Value::as_str)
                .map(str::to_string),
            expires_in,
            earliest_refresh_at: data.get("earliest_refresh_at").and_then(seconds),
        })
    }
}

/// A whole number of seconds, also from a JSON float.
fn seconds(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_f64().filter(|f| f.is_finite()).map(|f| f as i64))
}

fn has_plan_scope(scope: &str) -> bool {
    scope
        .split_whitespace()
        .any(|granted| granted == PLAN_SCOPE)
}

/// What DanyWhere keeps of a checked ID token.
struct IdClaims {
    subject: String,
    email: Option<String>,
    plan: Option<String>,
}

/// Checks issuer, audience, expiry, nonce (when given), authorized party and
/// subject; not the signature (see the module docs).
fn check_id_token(
    token: &str,
    client_id: &str,
    nonce: Option<&str>,
    now: i64,
) -> Result<IdClaims, ChatGptError> {
    let unreadable = || ChatGptError::Invalid("the ID token is unreadable".into());
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(unreadable());
    }
    let payload = URL_SAFE_NO_PAD
        .decode(parts[1].trim_end_matches('='))
        .map_err(|_| unreadable())?;
    let claims: Value = serde_json::from_slice(&payload).map_err(|_| unreadable())?;
    let text = |key: &str| claims.get(key).and_then(Value::as_str);

    let mut problems = Vec::new();
    if text("iss") != Some(ISSUER) {
        problems.push("iss");
    }
    let audience_ok = match claims.get("aud") {
        Some(Value::String(aud)) => aud == client_id,
        Some(Value::Array(auds)) => auds.iter().any(|aud| aud.as_str() == Some(client_id)),
        _ => false,
    };
    if !audience_ok {
        problems.push("aud");
    }
    if claims
        .get("exp")
        .and_then(seconds)
        .is_none_or(|exp| exp <= now - ID_TOKEN_LEEWAY_SECS)
    {
        problems.push("exp");
    }
    if let Some(nonce) = nonce {
        if text("nonce") != Some(nonce) {
            problems.push("nonce");
        }
    }
    if claims.get("azp").is_some() && text("azp") != Some(client_id) {
        problems.push("azp");
    }
    let subject = text("sub").filter(|sub| !sub.is_empty());
    if subject.is_none() {
        problems.push("sub");
    }
    if !problems.is_empty() {
        return Err(ChatGptError::Invalid(format!(
            "ID token check failed: {}",
            problems.join(", ")
        )));
    }
    Ok(IdClaims {
        subject: subject.unwrap_or_default().to_string(),
        email: text("email").map(str::to_string),
        plan: claims
            .get(AUTH_CLAIM)
            .and_then(|auth| auth.get("chatgpt_plan_type"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// The machine-readable code of a token endpoint error: `{"error": "code"}`
/// (OAuth) or `{"error": {"code": …}}`.
fn error_code(text: &str) -> Option<String> {
    let data: Value = serde_json::from_str(text).ok()?;
    match data.get("error")? {
        Value::String(code) => Some(code.clone()),
        Value::Object(error) => error
            .get("code")
            .or_else(|| error.get("type"))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

async fn post_form(
    client: &reqwest::Client,
    url: &str,
    fields: &[(&str, &str)],
) -> Result<(reqwest::StatusCode, String), ChatGptError> {
    let response = client
        .post(url)
        .header(CONTENT_TYPE, FORM)
        .header(ACCEPT, "application/json")
        .header(USER_AGENT, USER_AGENT_VALUE)
        .body(encode_pairs(fields))
        .send()
        .await
        .map_err(|e| ChatGptError::Http(transport_error("ChatGPT sign-in request failed", &e)))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| {
        ChatGptError::Http(transport_error(
            "Failed to read the ChatGPT sign-in answer",
            &e,
        ))
    })?;
    Ok((status, text))
}

fn status_error(status: reqwest::StatusCode, body: &str) -> String {
    format!("API request failed with status {status}: {body}")
}

/// `key=value&…`, every value percent-encoded (RFC 3986: only unreserved
/// characters stay, a space is `%20`).
fn encode_pairs(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// 32 random bytes, base64url without padding (43 characters).
fn random_value() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("No random bytes for the nonce: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// A transport error in `llm_client::report_reqwest_error`'s form, "context
/// (kind: …, url: …): caused by: …", with the URL's query cut off.
fn transport_error(context: &str, error: &reqwest::Error) -> String {
    let checks = [
        (error.is_builder(), "builder"),
        (error.is_connect(), "connect"),
        (error.is_request(), "request"),
        (error.is_redirect(), "redirect"),
        (error.is_timeout(), "timeout"),
        (error.is_status(), "status"),
        (error.is_body(), "body"),
        (error.is_decode(), "decode"),
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
        .map(|url| {
            let mut url = url.clone();
            url.set_query(None);
            url.set_fragment(None);
            let _ = url.set_password(None);
            format!(", url: {url}")
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const HOST: &str = "urn:uuid:3f2a6c1e-8d4b-4c7a-9e21-5b6d7f8a9c0d";
    const REDIRECT: &str = "http://127.0.0.1:1455/auth/callback";
    const ISSUED: &str = "oaiapp_issued_1";
    const NONCE: &str = "nonce-of-this-attempt";
    const FULL_SCOPE: &str =
        "chatgpt.tokens.use.direct email offline_access openid profile resource.invoke";

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn flow(client_id: Option<&str>) -> ChatGptFlow {
        ChatGptFlow {
            client_id: client_id.map(str::to_string),
            host_id: HOST.to_string(),
            agent_name: AGENT_NAME.to_string(),
            nonce: NONCE.to_string(),
        }
    }

    fn pkce() -> Pkce {
        Pkce {
            verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string(),
            challenge: "E9Melhoa2OwvFrEMTJguCQaoeYbMDJ7Jnsrn7_WVMqI".to_string(),
        }
    }

    fn code(client_id: Option<&str>) -> SignInCode {
        SignInCode {
            code: "auth-code-1".to_string(),
            redirect_uri: REDIRECT.to_string(),
            verifier: pkce().verifier,
            client_id: client_id.map(str::to_string),
        }
    }

    /// Decoded query pairs of a URL, in order.
    fn query(url: &str) -> Vec<(String, String)> {
        reqwest::Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    fn form(body: &str) -> Vec<(String, String)> {
        query(&format!("http://x/?{body}"))
    }

    fn get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
        pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn keys(pairs: &[(String, String)]) -> Vec<&str> {
        let mut keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort_unstable();
        keys
    }

    fn jwt(claims: &Value) -> String {
        let header = json!({"alg": "RS256", "kid": "key-1", "typ": "JWT"});
        format!(
            "{}.{}.c2lnbmF0dXJl",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    /// The claims of a good ID token, as the trial's token had them (aud as
    /// a list, no azp, the plan's metadata without a plan type).
    fn claims(client: &str) -> Value {
        json!({
            "iss": ISSUER, "aud": [client], "sub": "user-sub-1", "email": "jo@example.com",
            "email_verified": true, "exp": now() + 3600, "iat": now(), "auth_time": now(),
            "nonce": NONCE, "jti": "j", "sid": "s",
            "https://api.openai.com/auth": {"per_user_salt": "salt"}
        })
    }

    fn token_answer(id_token: Option<String>, scope: Option<&str>) -> String {
        let mut answer = json!({
            "access_token": "access-new", "refresh_token": "refresh-new",
            "token_type": "Bearer", "expires_in": 3600, "earliest_refresh_at": now() + 3240,
        });
        if let Some(token) = id_token {
            answer["id_token"] = json!(token);
        }
        if let Some(scope) = scope {
            answer["scope"] = json!(scope);
        }
        answer.to_string()
    }

    fn session() -> ChatGptSession {
        ChatGptSession {
            client_id: ISSUED.to_string(),
            access_token: "access-old".to_string(),
            refresh_token: "refresh-old".to_string(),
            expires_at: now() - 10,
            email: Some("jo@example.com".to_string()),
            plan: None,
            subject: "user-sub-1".to_string(),
            earliest_refresh_at: Some(now() - 370),
        }
    }

    // --- a local server ------------------------------------------------------

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

    /// Answers one request per entry, in turn, and keeps what it got.
    async fn serve(replies: Vec<(&'static str, String)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
        serve_with(|_| replies).await
    }

    /// [`serve`] with the replies `make` gives for the server's own URL.
    async fn serve_with(
        make: impl FnOnce(&str) -> Vec<(&'static str, String)>,
    ) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let replies = make(&format!("http://{address}"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        tokio::spawn(async move {
            for (status, body) in replies {
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
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        (format!("http://{address}"), seen)
    }

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    // --- the authorization URL -----------------------------------------------

    #[test]
    fn the_callback_is_127_0_0_1_on_auth_callback() {
        let spec = flow(None).callback();
        assert!(matches!(spec.host, CallbackHost::Ip127));
        assert_eq!(spec.path, "/auth/callback");
    }

    #[test]
    fn a_first_registration_asks_the_dynamic_client_with_name_hint_and_host() {
        let url = flow(None).authorize_url(REDIRECT, "state-1", &pkce());
        assert!(url.starts_with(&format!("{AUTHORIZE}?")), "{url}");
        let q = query(&url);
        assert_eq!(get(&q, "client_id"), Some("dynamic_agent_client"));
        assert_eq!(get(&q, "agent_name_hint"), Some("DanyWhere"));
        assert_eq!(get(&q, "ext_agent_host_id"), Some(HOST));
        assert_eq!(get(&q, "response_type"), Some("code"));
        assert_eq!(get(&q, "redirect_uri"), Some(REDIRECT));
        assert_eq!(
            get(&q, "scope").map(|s| s.split(' ').collect::<Vec<_>>()),
            Some(vec![
                "openid",
                "profile",
                "email",
                "offline_access",
                "resource.invoke",
                "chatgpt.tokens.use.direct"
            ])
        );
        assert_eq!(get(&q, "resource"), Some("https://api.openai.com/v1"));
        assert_eq!(get(&q, "state"), Some("state-1"));
        assert_eq!(get(&q, "nonce"), Some(NONCE));
        assert_eq!(get(&q, "code_challenge_method"), Some("S256"));
        assert_eq!(
            get(&q, "code_challenge"),
            Some("E9Melhoa2OwvFrEMTJguCQaoeYbMDJ7Jnsrn7_WVMqI")
        );
        assert_eq!(
            keys(&q),
            [
                "agent_name_hint",
                "client_id",
                "code_challenge",
                "code_challenge_method",
                "ext_agent_host_id",
                "nonce",
                "redirect_uri",
                "resource",
                "response_type",
                "scope",
                "state"
            ]
        );
        assert!(!url.contains("localhost"), "{url}");
        assert!(!url.contains(&pkce().verifier), "the verifier leaks: {url}");
    }

    #[test]
    fn a_reauthorization_uses_the_issued_client_without_name_hint() {
        let url = flow(Some("oaiapp_saved")).authorize_url(REDIRECT, "s", &pkce());
        let q = query(&url);
        assert_eq!(get(&q, "client_id"), Some("oaiapp_saved"));
        assert_eq!(get(&q, "agent_name_hint"), None);
        assert_eq!(get(&q, "ext_agent_host_id"), Some(HOST));
        assert_eq!(get(&q, "nonce"), Some(NONCE));
    }

    #[test]
    fn the_url_values_are_percent_encoded() {
        let url = flow(None).authorize_url(REDIRECT, "a+b c&d=e/f", &pkce());
        assert!(
            url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A1455%2Fauth%2Fcallback"),
            "{url}"
        );
        assert!(
            url.contains("scope=openid%20profile%20email%20offline_access%20resource.invoke%20chatgpt.tokens.use.direct"),
            "{url}"
        );
        assert!(url.contains("ext_agent_host_id=urn%3Auuid%3A"), "{url}");
        assert_eq!(get(&query(&url), "state"), Some("a+b c&d=e/f"));
    }

    #[test]
    fn a_new_flow_has_a_fresh_nonce_and_the_app_name() {
        let first = ChatGptFlow::new(None, HOST.to_string()).unwrap();
        let second = ChatGptFlow::new(Some(ISSUED.to_string()), HOST.to_string()).unwrap();
        assert!(first.nonce.len() >= 43, "nonce {:?}", first.nonce);
        assert_ne!(first.nonce, second.nonce);
        assert_eq!(first.agent_name, "DanyWhere");
        assert_eq!(second.client_id.as_deref(), Some(ISSUED));
        assert_eq!(first.host_id, HOST);
    }

    #[test]
    fn a_host_id_is_a_random_version_4_urn_uuid() {
        let id = new_host_id().unwrap();
        let uuid = id.strip_prefix("urn:uuid:").expect("urn:uuid: prefix");
        let groups: Vec<&str> = uuid.split('-').collect();
        assert_eq!(
            groups.iter().map(|g| g.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12],
            "{id}"
        );
        assert!(uuid
            .chars()
            .all(|c| c == '-' || c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert!(groups[2].starts_with('4'), "version 4: {id}");
        assert!(
            matches!(groups[3].chars().next(), Some('8' | '9' | 'a' | 'b')),
            "RFC 4122 variant: {id}"
        );
        assert_ne!(new_host_id().unwrap(), id);
    }

    // --- the code exchange ---------------------------------------------------

    #[tokio::test]
    async fn the_code_exchange_posts_the_documented_form() {
        let answer = token_answer(Some(jwt(&claims(ISSUED))), Some(FULL_SCOPE));
        let (base, seen) = serve(vec![("200 OK", answer)]).await;
        let session = exchange_code(
            &client(),
            &format!("{base}/api/accounts/oauth/token"),
            &flow(None),
            &code(Some(ISSUED)),
        )
        .await
        .expect("a session");

        let seen = seen.lock().unwrap()[0].clone();
        assert!(
            seen.head
                .starts_with("POST /api/accounts/oauth/token HTTP/1.1"),
            "{}",
            seen.head
        );
        assert_eq!(
            seen.header("content-type").as_deref(),
            Some("application/x-www-form-urlencoded")
        );
        assert!(seen
            .header("user-agent")
            .is_some_and(|agent| agent.starts_with("DanyWhere/")));
        let sent = form(&seen.body);
        assert_eq!(
            keys(&sent),
            [
                "client_id",
                "code",
                "code_verifier",
                "grant_type",
                "redirect_uri",
                "resource"
            ]
        );
        assert_eq!(get(&sent, "grant_type"), Some("authorization_code"));
        assert_eq!(get(&sent, "client_id"), Some(ISSUED));
        assert_eq!(get(&sent, "code"), Some("auth-code-1"));
        assert_eq!(get(&sent, "code_verifier"), Some(pkce().verifier.as_str()));
        assert_eq!(get(&sent, "redirect_uri"), Some(REDIRECT));
        assert_eq!(get(&sent, "resource"), Some(RESOURCE));

        assert_eq!(session.client_id, ISSUED);
        assert_eq!(session.access_token, "access-new");
        assert_eq!(session.refresh_token, "refresh-new");
        assert!(
            (now() + 3590..=now() + 3610).contains(&session.expires_at),
            "expires_at {}",
            session.expires_at
        );
        assert_eq!(session.email.as_deref(), Some("jo@example.com"));
        assert_eq!(session.subject, "user-sub-1");
        assert_eq!(session.plan, None);
        assert!(session.earliest_refresh_at.is_some());
    }

    #[tokio::test]
    async fn the_plan_type_comes_along_when_the_id_token_names_it() {
        let mut with_plan = claims(ISSUED);
        with_plan["https://api.openai.com/auth"]["chatgpt_plan_type"] = json!("plus");
        let (base, _) = serve(vec![(
            "200 OK",
            token_answer(Some(jwt(&with_plan)), Some(FULL_SCOPE)),
        )])
        .await;
        let session = exchange_code(&client(), &base, &flow(None), &code(Some(ISSUED)))
            .await
            .expect("a session");
        assert_eq!(session.plan.as_deref(), Some("plus"));
    }

    #[tokio::test]
    async fn a_first_registration_without_an_issued_client_is_incomplete() {
        let (base, seen) = serve(Vec::new()).await;
        for callback_client in [None, Some(DYNAMIC_CLIENT), Some("")] {
            let result = exchange_code(&client(), &base, &flow(None), &code(callback_client)).await;
            assert!(
                matches!(result, Err(ChatGptError::Invalid(_))),
                "{callback_client:?}: {result:?}"
            );
        }
        assert!(seen.lock().unwrap().is_empty(), "no code may be spent");
    }

    #[tokio::test]
    async fn a_reauthorization_keeps_the_saved_client() {
        let answer = token_answer(Some(jwt(&claims("oaiapp_saved"))), Some(FULL_SCOPE));
        let (base, seen) = serve(vec![("200 OK", answer.clone()), ("200 OK", answer)]).await;
        for callback_client in [None, Some("oaiapp_saved")] {
            let session = exchange_code(
                &client(),
                &base,
                &flow(Some("oaiapp_saved")),
                &code(callback_client),
            )
            .await
            .expect("a session");
            assert_eq!(session.client_id, "oaiapp_saved");
        }
        for request in seen.lock().unwrap().iter() {
            assert_eq!(get(&form(&request.body), "client_id"), Some("oaiapp_saved"));
        }
    }

    /// Review of 2026-11-27, M3: OpenAI issued a new client for a saved one
    /// it no longer knows; the new one is the one to use from now on.
    #[tokio::test]
    async fn a_callback_naming_a_newly_issued_client_takes_it() {
        let answer = token_answer(Some(jwt(&claims("oaiapp_other"))), Some(FULL_SCOPE));
        let (base, seen) = serve(vec![("200 OK", answer)]).await;
        let session = exchange_code(
            &client(),
            &base,
            &flow(Some("oaiapp_saved")),
            &code(Some("oaiapp_other")),
        )
        .await
        .expect("a session");
        assert_eq!(session.client_id, "oaiapp_other");
        let sent = seen.lock().unwrap()[0].clone();
        assert_eq!(get(&form(&sent.body), "client_id"), Some("oaiapp_other"));
    }

    /// Review of 2026-11-27, M3: whether an error says the client itself is
    /// gone, so the next sign-in registers anew.
    #[test]
    fn a_dead_client_is_told_from_a_dead_refresh_token() {
        assert!(names_dead_client(
            &ChatGptError::SignInExpired("invalid_client".to_string())
        ));
        assert!(names_dead_client(&ChatGptError::Http(status_error(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":"invalid_client","error_description":"Unknown client"}"#
        ))));
        assert!(!names_dead_client(
            &ChatGptError::SignInExpired("invalid_grant".to_string())
        ));
        assert!(!names_dead_client(&ChatGptError::Http(status_error(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":"invalid_grant"}"#
        ))));
        assert!(!names_dead_client(&ChatGptError::NotEligible));
    }

    #[tokio::test]
    async fn the_id_token_is_checked() {
        let mut cases: Vec<(&str, Option<String>)> = Vec::new();
        let mut wrong_aud = claims(ISSUED);
        wrong_aud["aud"] = json!(["oaiapp_someone_else"]);
        cases.push(("aud", Some(jwt(&wrong_aud))));
        let mut expired = claims(ISSUED);
        expired["exp"] = json!(now() - 3600);
        cases.push(("exp", Some(jwt(&expired))));
        let mut wrong_nonce = claims(ISSUED);
        wrong_nonce["nonce"] = json!("an-old-nonce");
        cases.push(("nonce", Some(jwt(&wrong_nonce))));
        let mut no_nonce = claims(ISSUED);
        no_nonce.as_object_mut().unwrap().remove("nonce");
        cases.push(("nonce missing", Some(jwt(&no_nonce))));
        let mut wrong_iss = claims(ISSUED);
        wrong_iss["iss"] = json!("https://auth.example.com");
        cases.push(("iss", Some(jwt(&wrong_iss))));
        let mut no_sub = claims(ISSUED);
        no_sub.as_object_mut().unwrap().remove("sub");
        cases.push(("sub", Some(jwt(&no_sub))));
        let mut other_azp = claims(ISSUED);
        other_azp["azp"] = json!("oaiapp_someone_else");
        cases.push(("azp", Some(jwt(&other_azp))));
        cases.push(("unreadable", Some("not-a-token".to_string())));
        cases.push(("missing", None));

        let replies = cases
            .iter()
            .map(|(_, token)| ("200 OK", token_answer(token.clone(), Some(FULL_SCOPE))))
            .collect();
        let (base, _) = serve(replies).await;
        for (name, _) in &cases {
            let result = exchange_code(&client(), &base, &flow(None), &code(Some(ISSUED))).await;
            assert!(
                matches!(result, Err(ChatGptError::Invalid(_))),
                "{name}: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_audience_given_as_a_string_is_accepted() {
        let mut plain = claims(ISSUED);
        plain["aud"] = json!(ISSUED);
        let (base, _) = serve(vec![(
            "200 OK",
            token_answer(Some(jwt(&plain)), Some(FULL_SCOPE)),
        )])
        .await;
        let result = exchange_code(&client(), &base, &flow(None), &code(Some(ISSUED))).await;
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn without_the_plan_scope_the_sign_in_is_not_eligible() {
        let (base, _) = serve(vec![(
            "200 OK",
            token_answer(
                Some(jwt(&claims(ISSUED))),
                Some("email offline_access openid profile resource.invoke"),
            ),
        )])
        .await;
        let result = exchange_code(&client(), &base, &flow(None), &code(Some(ISSUED))).await;
        assert_eq!(result.unwrap_err(), ChatGptError::NotEligible);
    }

    #[tokio::test]
    async fn a_token_answer_without_refresh_token_or_access_token_is_invalid() {
        let mut no_refresh: Value =
            serde_json::from_str(&token_answer(Some(jwt(&claims(ISSUED))), Some(FULL_SCOPE)))
                .unwrap();
        no_refresh.as_object_mut().unwrap().remove("refresh_token");
        let mut no_access = no_refresh.clone();
        no_access["refresh_token"] = json!("r");
        no_access.as_object_mut().unwrap().remove("access_token");
        let mut no_bearer = no_access.clone();
        no_bearer["access_token"] = json!("a");
        no_bearer["token_type"] = json!("mac");
        let (base, _) = serve(vec![
            ("200 OK", no_refresh.to_string()),
            ("200 OK", no_access.to_string()),
            ("200 OK", no_bearer.to_string()),
        ])
        .await;
        for name in ["no refresh token", "no access token", "not bearer"] {
            let result = exchange_code(&client(), &base, &flow(None), &code(Some(ISSUED))).await;
            assert!(
                matches!(result, Err(ChatGptError::Invalid(_))),
                "{name}: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_refused_code_keeps_status_and_body() {
        let body = r#"{"error":"invalid_grant","error_description":"Code expired"}"#;
        let (base, _) = serve(vec![("400 Bad Request", body.to_string())]).await;
        let result = exchange_code(&client(), &base, &flow(None), &code(Some(ISSUED))).await;
        assert_eq!(
            result.unwrap_err(),
            ChatGptError::Http(format!(
                "API request failed with status 400 Bad Request: {body}"
            ))
        );
    }

    // --- refresh -------------------------------------------------------------

    #[tokio::test]
    async fn a_refresh_posts_the_documented_form_and_takes_the_new_tokens() {
        let (base, seen) = serve(vec![("200 OK", token_answer(None, None))]).await;
        let old = session();
        let new = refresh(&client(), &format!("{base}/api/accounts/oauth/token"), &old)
            .await
            .expect("a new session");

        let seen = seen.lock().unwrap()[0].clone();
        assert!(seen
            .head
            .starts_with("POST /api/accounts/oauth/token HTTP/1.1"));
        let sent = form(&seen.body);
        assert_eq!(
            keys(&sent),
            ["client_id", "grant_type", "refresh_token", "resource"],
            "no scope, so the grant stays"
        );
        assert_eq!(get(&sent, "grant_type"), Some("refresh_token"));
        assert_eq!(get(&sent, "client_id"), Some(ISSUED));
        assert_eq!(get(&sent, "refresh_token"), Some("refresh-old"));
        assert_eq!(get(&sent, "resource"), Some(RESOURCE));

        assert_eq!(new.access_token, "access-new");
        assert_eq!(new.refresh_token, "refresh-new");
        assert!((now() + 3590..=now() + 3610).contains(&new.expires_at));
        assert!(new.earliest_refresh_at.is_some_and(|at| at > now()));
        assert_eq!(new.client_id, old.client_id);
        assert_eq!(new.subject, old.subject);
        assert_eq!(new.email, old.email);
    }

    #[tokio::test]
    async fn a_refresh_with_an_id_token_of_the_same_account_takes_its_email() {
        let mut renewed = claims(ISSUED);
        renewed.as_object_mut().unwrap().remove("nonce");
        renewed["email"] = json!("jo.new@example.com");
        let (base, _) = serve(vec![(
            "200 OK",
            token_answer(Some(jwt(&renewed)), Some(FULL_SCOPE)),
        )])
        .await;
        let new = refresh(&client(), &base, &session())
            .await
            .expect("a session");
        assert_eq!(new.email.as_deref(), Some("jo.new@example.com"));
    }

    #[tokio::test]
    async fn a_refresh_with_an_id_token_of_another_account_is_rejected() {
        let mut other = claims(ISSUED);
        other["sub"] = json!("someone-else");
        let (base, _) = serve(vec![("200 OK", token_answer(Some(jwt(&other)), None))]).await;
        let result = refresh(&client(), &base, &session()).await;
        assert!(
            matches!(result, Err(ChatGptError::Invalid(_))),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn dead_refresh_tokens_mean_the_sign_in_expired() {
        let codes = [
            "invalid_grant",
            "invalid_refresh_token",
            "token_expired",
            "refresh_token_expired",
            "refresh_token_invalidated",
            "refresh_token_reused",
            "invalid_client",
        ];
        let mut replies: Vec<(&'static str, String)> = codes
            .iter()
            .map(|code| {
                (
                    "400 Bad Request",
                    json!({"error": code, "error_description": "gone"}).to_string(),
                )
            })
            .collect();
        replies.push((
            "401 Unauthorized",
            json!({"error": {"code": "refresh_token_reused", "message": "reused"}}).to_string(),
        ));
        let (base, _) = serve(replies).await;
        for code in codes.iter().chain(["refresh_token_reused"].iter()) {
            let result = refresh(&client(), &base, &session()).await;
            assert_eq!(
                result.unwrap_err(),
                ChatGptError::SignInExpired(code.to_string()),
                "{code}"
            );
        }
    }

    #[tokio::test]
    async fn a_refresh_that_fails_for_now_keeps_the_sign_in() {
        let (base, _) = serve(vec![
            ("503 Service Unavailable", "upstream down".to_string()),
            (
                "500 Internal Server Error",
                json!({"error": {"code": "server_error"}}).to_string(),
            ),
        ])
        .await;
        let first = refresh(&client(), &base, &session()).await.unwrap_err();
        assert_eq!(
            first,
            ChatGptError::Http(
                "API request failed with status 503 Service Unavailable: upstream down".to_string()
            )
        );
        let second = refresh(&client(), &base, &session()).await.unwrap_err();
        assert!(matches!(second, ChatGptError::Http(_)), "{second:?}");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let offline = refresh(&client(), &format!("http://{address}"), &session())
            .await
            .unwrap_err();
        match offline {
            ChatGptError::Http(text) => {
                assert!(
                    text.contains("(kind: ") && text.contains("connect"),
                    "{text}"
                );
                assert!(!text.contains("refresh-old"), "leaks the token: {text}");
            }
            other => panic!("expected Http, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_refresh_answer_without_the_plan_scope_is_not_eligible() {
        let (base, _) = serve(vec![(
            "200 OK",
            token_answer(None, Some("openid profile email offline_access")),
        )])
        .await;
        let result = refresh(&client(), &base, &session()).await;
        assert_eq!(result.unwrap_err(), ChatGptError::NotEligible);
    }

    #[tokio::test]
    async fn a_refresh_answer_without_a_new_refresh_token_keeps_the_old_one() {
        let mut answer: Value = serde_json::from_str(&token_answer(None, None)).unwrap();
        answer.as_object_mut().unwrap().remove("refresh_token");
        let (base, _) = serve(vec![("200 OK", answer.to_string())]).await;
        let new = refresh(&client(), &base, &session())
            .await
            .expect("a session");
        assert_eq!(new.refresh_token, "refresh-old");
        assert_eq!(new.access_token, "access-new");
    }

    // --- when to refresh -----------------------------------------------------

    #[test]
    fn a_refresh_is_due_five_minutes_before_the_end() {
        let at = 1_800_000_000;
        let mut s = session();
        s.earliest_refresh_at = None;
        s.expires_at = at + 301;
        assert!(!needs_refresh(&s, at), "301 s left");
        s.expires_at = at + 299;
        assert!(needs_refresh(&s, at), "299 s left");
        s.expires_at = at - 10;
        assert!(needs_refresh(&s, at), "expired");
    }

    #[test]
    fn a_refresh_waits_for_earliest_refresh_at_while_the_token_still_works() {
        let at = 1_800_000_000;
        let mut s = session();
        s.expires_at = at + 200;
        s.earliest_refresh_at = Some(at + 60);
        assert!(!needs_refresh(&s, at), "too early, token still valid");
        s.earliest_refresh_at = Some(at - 1);
        assert!(needs_refresh(&s, at), "earliest time passed");
        s.expires_at = at - 1;
        s.earliest_refresh_at = Some(at + 60);
        assert!(needs_refresh(&s, at), "expired: refresh anyway");
    }

    // --- what the session shows ----------------------------------------------

    #[test]
    fn the_debug_output_hides_the_tokens() {
        let text = format!("{:?}", session());
        assert!(!text.contains("access-old"), "{text}");
        assert!(!text.contains("refresh-old"), "{text}");
        assert!(text.contains(ISSUED), "{text}");
    }

    #[test]
    fn an_expired_sign_in_reads_as_sign_in_expired() {
        let text = ChatGptError::SignInExpired("refresh_token_expired".to_string()).to_string();
        assert!(text.starts_with("sign-in-expired: "), "{text}");
        assert!(text.contains("refresh_token_expired"), "{text}");
        let http = ChatGptError::Http("API request failed with status 500 x: y".to_string());
        assert_eq!(http.to_string(), "API request failed with status 500 x: y");
        assert!(ChatGptError::NotEligible
            .to_string()
            .starts_with("plan-not-eligible: "));
    }

    #[test]
    fn a_session_stored_before_the_added_fields_still_reads() {
        let stored = json!({"client_id": ISSUED, "access_token": "a", "refresh_token": "r",
                            "expires_at": 1_800_000_000_i64, "email": null, "plan": null});
        let session: ChatGptSession = serde_json::from_value(stored).expect("reads");
        assert_eq!(session.subject, "");
        assert_eq!(session.earliest_refresh_at, None);
    }

    // --- models --------------------------------------------------------------

    #[tokio::test]
    async fn the_model_list_keeps_the_listed_slugs_in_server_order() {
        let catalog = json!({"models": [
            {"slug": "gpt-6.2-sol", "display_name": "GPT-6.2 Sol", "visibility": "list"},
            {"slug": "gpt-internal", "display_name": "x", "visibility": "hide"},
            {"slug": "gpt-6-luna", "display_name": "GPT-6 Luna", "visibility": "list"},
            {"display_name": "no slug", "visibility": "list"}
        ]});
        let (base, seen) = serve(vec![("200 OK", catalog.to_string())]).await;
        let models = list_models(&client(), &format!("{base}/v1"), &session()).await;
        assert_eq!(
            models,
            Ok(vec!["gpt-6.2-sol".to_string(), "gpt-6-luna".to_string()])
        );
        let seen = seen.lock().unwrap()[0].clone();
        assert!(
            seen.head.starts_with("GET /v1/models HTTP/1.1"),
            "{}",
            seen.head
        );
        assert_eq!(
            seen.header("authorization").as_deref(),
            Some("Bearer access-old")
        );
    }

    #[tokio::test]
    async fn a_refused_model_list_keeps_status_and_body() {
        let body = r#"{"error":{"code":"subscription_sharing_invalid_user","message":"no"}}"#;
        let (base, _) = serve(vec![("401 Unauthorized", body.to_string())]).await;
        let result = list_models(&client(), &base, &session()).await;
        assert_eq!(
            result,
            Err(format!(
                "API request failed with status 401 Unauthorized: {body}"
            ))
        );
    }

    // --- signing out ----------------------------------------------------------

    fn discovery(revocation_endpoint: &str) -> String {
        json!({
            "issuer": "https://auth.openai.com",
            "token_endpoint": "https://auth.openai.com/api/accounts/oauth/token",
            "revocation_endpoint": revocation_endpoint,
            "revocation_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post", "none"],
        })
        .to_string()
    }

    #[tokio::test]
    async fn revoking_posts_the_refresh_token_to_the_discovered_endpoint() {
        let (base, seen) = serve_with(|base| {
            vec![
                (
                    "200 OK",
                    discovery(&format!("{base}/api/accounts/oauth/revoke")),
                ),
                ("200 OK", String::new()),
            ]
        })
        .await;
        let result = revoke(
            &client(),
            &format!("{base}/.well-known/openid-configuration"),
            &session(),
        )
        .await;
        assert_eq!(result, Ok(()));
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert!(
            seen[0]
                .head
                .starts_with("GET /.well-known/openid-configuration HTTP/1.1"),
            "{}",
            seen[0].head
        );
        assert!(
            seen[1]
                .head
                .starts_with("POST /api/accounts/oauth/revoke HTTP/1.1"),
            "{}",
            seen[1].head
        );
        assert_eq!(seen[1].header("content-type").as_deref(), Some(FORM));
        let fields = form(&seen[1].body);
        assert_eq!(get(&fields, "token"), Some("refresh-old"));
        assert_eq!(get(&fields, "token_type_hint"), Some("refresh_token"));
        assert_eq!(get(&fields, "client_id"), Some(ISSUED));
        assert_eq!(keys(&fields), vec!["client_id", "token", "token_type_hint"]);
        // No bearer: the refresh token goes only in the form.
        assert_eq!(seen[1].header("authorization"), None);
    }

    #[tokio::test]
    async fn a_refused_revocation_is_an_error_in_llm_clients_form() {
        let (base, _) = serve_with(|base| {
            vec![
                (
                    "200 OK",
                    discovery(&format!("{base}/api/accounts/oauth/revoke")),
                ),
                ("503 Service Unavailable", "busy".to_string()),
            ]
        })
        .await;
        let result = revoke(
            &client(),
            &format!("{base}/.well-known/openid-configuration"),
            &session(),
        )
        .await;
        assert_eq!(
            result,
            Err("API request failed with status 503 Service Unavailable: busy".to_string())
        );
    }

    #[tokio::test]
    async fn the_refresh_token_never_goes_to_another_origin() {
        let (base, seen) = serve(vec![(
            "200 OK",
            discovery("https://elsewhere.invalid/api/accounts/oauth/revoke"),
        )])
        .await;
        let result = revoke(
            &client(),
            &format!("{base}/.well-known/openid-configuration"),
            &session(),
        )
        .await;
        let error = result.expect_err("refused");
        assert!(!error.contains("refresh-old"), "{error}");
        assert_eq!(seen.lock().unwrap().len(), 1, "only the discovery");
    }

    #[tokio::test]
    async fn a_discovery_without_a_revocation_endpoint_is_an_error() {
        let (base, seen) = serve(vec![(
            "200 OK",
            json!({"issuer": "https://auth.openai.com"}).to_string(),
        )])
        .await;
        let result = revoke(
            &client(),
            &format!("{base}/.well-known/openid-configuration"),
            &session(),
        )
        .await;
        assert!(result.is_err(), "{result:?}");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_documented_endpoints_and_defaults() {
        assert_eq!(AUTHORIZE, "https://auth.openai.com/api/accounts/authorize");
        assert_eq!(TOKEN, "https://auth.openai.com/api/accounts/oauth/token");
        assert_eq!(
            DISCOVERY,
            "https://auth.openai.com/.well-known/openid-configuration"
        );
        assert_eq!(DEFAULT_MODEL, "gpt-6.2-sol");
        assert_eq!(DEFAULT_EFFORT, "low");
        assert_eq!(USAGE_URL, "https://chatgpt.com/settings/usage");
    }

    /// Review of 2026-11-27, N18: OpenAI admits open-source apps, so the
    /// sign-in names DanyWhere's own repository.
    #[test]
    fn the_sign_in_names_danywheres_repository() {
        assert_eq!(
            USER_AGENT_VALUE,
            "DanyWhere/1.0 (+https://github.com/yaccob/DanyWhere)"
        );
    }
}
