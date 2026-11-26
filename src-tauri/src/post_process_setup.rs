//! DanyWhere: the post-processing setup (user, 2026-11-26; sidecar notes
//! `2026-11-26_1840_nachbearbeitung-einrichtung-mit-fragen` and
//! `2026-11-26_2250_nachbearbeitung-einstieg-umsetzung`, section 4.3).
//!
//! Instead of choosing a provider, the user answers a few questions ("Where
//! may your texts go?", the subscription, …) and gets one way suggested
//! (`src/lib/postProcessSetup/questions.ts`). What is stored here:
//!
//! - the answers and whether the setup was finished or put off
//!   ([`PostProcessSetupState`], `AppSettings::post_process_setup`);
//! - the sign-in with ChatGPT ([`ChatGptSessionStored`]) and this
//!   installation's id there, and what OpenRouter said about the key of its
//!   sign-in ([`OpenRouterConnection`]).
//!
//! These are global, never profile values or part of a profile's export, and
//! their secrets stay out of the debug output. The provider, model, key and
//! effort a way sets go into the defaults (`profile_layers`), so every
//! profile that follows them post-processes that way.
//!
//! Whether post-processing can run is [`post_process_ready`]: a provider, its
//! model and its access (an API key, the sign-in, or nothing for a server of
//! one's own and Apple Intelligence). A dictation that asks for
//! post-processing without it goes out unprocessed with the notice that says
//! what is missing (`no-provider`, `no-model`, `no-api-key`; `actions`).

use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

use crate::post_process_failure::{FailureReason, PostProcessFailure};
use crate::profile_layers::{self, Scope};
use crate::settings::{self, AppSettings, PostProcessProvider, APPLE_INTELLIGENCE_PROVIDER_ID};

// ---- What is stored --------------------------------------------------------

/// Question 1: where the texts may go.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum TextsMayGo {
    /// Also to free services, which may keep or learn from them.
    FreeServices,
    /// Only to a service that doesn't keep them.
    NotKept,
    /// Nowhere: only this computer.
    Nowhere,
}

/// Question 2: the subscription the user already has.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum Subscription {
    Chatgpt,
    Claude,
    Google,
    Huggingface,
    Ollama,
    None,
}

/// Question 3, without a subscription: what to do next.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum NextStep {
    /// Top up OpenRouter once.
    Credit,
    /// Groq's free plan with a key.
    Groq,
    /// A language model on this computer.
    ThisComputer,
}

/// The answers so far, in the names of `questions.ts` (snake_case there too).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default, Type)]
#[serde(default)]
pub struct SetupAnswers {
    pub texts_may_go: Option<TextsMayGo>,
    pub subscription: Option<Subscription>,
    pub next_step: Option<NextStep>,
}

/// The setup's state: the answers, whether it was finished, whether the
/// user put it off ("Later").
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default, Type)]
#[serde(default)]
pub struct PostProcessSetupState {
    pub answers: Option<SetupAnswers>,
    pub finished: bool,
    pub skipped: bool,
}

/// The sign-in with ChatGPT, as stored: the fields of strand C's
/// `ChatGptSession`, one to one, each with a default so a store from before
/// one of them loads.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default, Type)]
#[serde(default)]
pub struct ChatGptSessionStored {
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: String,
    /// When the access token expires, in seconds since the Unix epoch.
    pub expires_at: i64,
    /// Not refreshed before this, in seconds since the Unix epoch.
    pub earliest_refresh_at: Option<i64>,
    /// The account's id (`sub` of the ID token).
    pub subject: Option<String>,
    pub email: Option<String>,
    pub plan: Option<String>,
}

/// What the debug output shows of a secret.
const REDACTED: &str = "[REDACTED]";

/// `text` for the debug output: empty stays empty, anything else is redacted.
fn redacted(text: &str) -> &'static str {
    if text.is_empty() {
        ""
    } else {
        REDACTED
    }
}

impl fmt::Debug for ChatGptSessionStored {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatGptSessionStored")
            .field("client_id", &self.client_id)
            .field("access_token", &redacted(&self.access_token))
            .field("refresh_token", &redacted(&self.refresh_token))
            .field("expires_at", &self.expires_at)
            .field("earliest_refresh_at", &self.earliest_refresh_at)
            .field("subject", &self.subject.as_deref().map(redacted))
            .field("email", &self.email.as_deref().map(redacted))
            .field("plan", &self.plan)
            .finish()
    }
}

/// A text that stays out of the debug output (this installation's id at
/// ChatGPT's sign-in). Stored as a plain string.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Type)]
#[serde(transparent)]
pub struct SecretText(pub String);

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(redacted(&self.0))
    }
}

/// What OpenRouter said about the key of its sign-in (`GET /api/v1/key`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct OpenRouterConnection {
    /// No credit was ever bought: only the free models.
    pub free_tier: bool,
    /// The key's credit limit in US dollars; `None`: unlimited.
    #[serde(default)]
    pub limit_usd: Option<f64>,
    /// The free models to try, in order (the request's `models`).
    #[serde(default)]
    pub free_models: Vec<String>,
    /// When it was asked, in seconds since the Unix epoch.
    pub checked_at: i64,
}

// ---- Whether post-processing can run ----------------------------------------

/// Whether post-processing can run, or what it lacks.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    /// No provider is selected (or it doesn't exist).
    Provider,
    /// The provider has no model.
    Model,
    /// The provider needs an API key and has none.
    Key,
    /// The provider needs a sign-in (ChatGPT) and has none.
    SignIn,
}

impl Readiness {
    /// The reason a dictation's notice names for what is missing; `None`
    /// when ready. A missing sign-in is set up as a missing key is.
    pub fn failure_reason(self) -> Option<FailureReason> {
        match self {
            Readiness::Ready => None,
            Readiness::Provider => Some(FailureReason::NoProvider),
            Readiness::Model => Some(FailureReason::NoModel),
            Readiness::Key | Readiness::SignIn => Some(FailureReason::NoApiKey),
        }
    }
}

/// The provider whose plan post-processes after the sign-in with ChatGPT.
pub const CHATGPT_PROVIDER_ID: &str = "chatgpt";
/// The provider OpenRouter's sign-in sets up.
pub const OPENROUTER_PROVIDER_ID: &str = "openrouter";
/// The provider of a server of one's own.
pub const CUSTOM_PROVIDER_ID: &str = "custom";
/// Ollama's cloud, whose models are listed by their tags.
pub const OLLAMA_CLOUD_PROVIDER_ID: &str = "ollama_cloud";

/// Whether provider `provider_id` needs an API key: not a server of one's
/// own, Apple Intelligence or ChatGPT's plan (a sign-in).
pub fn needs_key(provider_id: &str) -> bool {
    !matches!(
        provider_id,
        CUSTOM_PROVIDER_ID | APPLE_INTELLIGENCE_PROVIDER_ID | CHATGPT_PROVIDER_ID
    )
}

/// Whether `settings` (with the values of the profile that applies) can
/// post-process: a provider, its model and its access.
pub fn post_process_ready(settings: &AppSettings) -> Readiness {
    let Some(provider) = settings.active_post_process_provider() else {
        return Readiness::Provider;
    };
    let model = settings
        .post_process_models
        .get(&provider.id)
        .map(|model| model.trim())
        .unwrap_or_default();
    if model.is_empty() {
        return Readiness::Model;
    }
    if provider.id == CHATGPT_PROVIDER_ID {
        return if settings.chatgpt_signed_in {
            Readiness::Ready
        } else {
            Readiness::SignIn
        };
    }
    let key = settings
        .post_process_api_keys
        .get(&provider.id)
        .map(|key| key.trim())
        .unwrap_or_default();
    if needs_key(&provider.id) && key.is_empty() {
        return Readiness::Key;
    }
    Readiness::Ready
}

/// The way post-processing is set up, as the setup names it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetupWay {
    /// OpenRouter through its sign-in: its free models, or with credit.
    Openrouter {
        free_tier: bool,
        /// The key's credit limit in US dollars; `None`: unlimited.
        limit_usd: Option<f64>,
    },
    /// ChatGPT's plan, through its sign-in: the account, as far as known.
    ChatgptPlan {
        plan: Option<String>,
        email: Option<String>,
    },
    /// A provider's API key, pasted in.
    GuidedKey { provider: String },
    /// A server of one's own (the provider `custom`, e.g. Ollama here).
    OwnServer,
    /// Apple Intelligence on this Mac.
    OnDevice,
}

/// What the setup shows of the ChatGPT sign-in: never its tokens. It comes
/// from the sign-in's own file (`post_process_signin::vault`), not from the
/// settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatGptAccount {
    pub email: Option<String>,
    pub plan: Option<String>,
}

/// How `settings` post-process, when they can ([`post_process_ready`]);
/// `account` is the ChatGPT sign-in's.
pub fn current_way(settings: &AppSettings, account: Option<&ChatGptAccount>) -> Option<SetupWay> {
    if post_process_ready(settings) != Readiness::Ready {
        return None;
    }
    let provider = settings.active_post_process_provider()?;
    Some(match provider.id.as_str() {
        OPENROUTER_PROVIDER_ID => match &settings.openrouter_connection {
            Some(connection) => SetupWay::Openrouter {
                free_tier: connection.free_tier,
                limit_usd: connection.limit_usd,
            },
            None => SetupWay::GuidedKey {
                provider: provider.id.clone(),
            },
        },
        CHATGPT_PROVIDER_ID => SetupWay::ChatgptPlan {
            plan: account.and_then(|account| account.plan.clone()),
            email: account.and_then(|account| account.email.clone()),
        },
        CUSTOM_PROVIDER_ID => SetupWay::OwnServer,
        APPLE_INTELLIGENCE_PROVIDER_ID => SetupWay::OnDevice,
        id => SetupWay::GuidedKey {
            provider: id.to_string(),
        },
    })
}

/// `settings` with the defaults' values in their flat fields: what the
/// setup writes and the sample runs with.
pub fn with_defaults(settings: &AppSettings) -> AppSettings {
    let mut layer = settings.clone();
    crate::profiles::default_values(settings).apply_to(&mut layer);
    layer
}

/// What the setup's page shows.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct SetupStatus {
    /// Of the defaults.
    pub readiness: Readiness,
    /// The defaults' provider, if it exists.
    pub provider_id: Option<String>,
    /// Its model, if one is chosen.
    pub model: Option<String>,
    /// How it is set up, once it is ready.
    pub way: Option<SetupWay>,
    /// This computer's memory (`system_memory`), for the local model.
    pub memory_bytes: u64,
    /// The answers and whether the setup was finished or put off.
    pub setup: PostProcessSetupState,
}

/// The status of `settings`' defaults, with `memory_bytes` and the ChatGPT
/// sign-in's `account`.
pub fn status(
    settings: &AppSettings,
    memory_bytes: u64,
    account: Option<&ChatGptAccount>,
) -> SetupStatus {
    let layer = with_defaults(settings);
    let provider = layer.active_post_process_provider();
    SetupStatus {
        readiness: post_process_ready(&layer),
        provider_id: provider.map(|provider| provider.id.clone()),
        model: provider
            .and_then(|provider| layer.post_process_models.get(&provider.id))
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty()),
        way: current_way(&layer, account),
        memory_bytes,
        setup: settings.post_process_setup.clone(),
    }
}

// ---- Applying a way -----------------------------------------------------------

/// What a way sets in the defaults.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct SetupChoice {
    pub provider_id: String,
    /// `None` or empty: the provider's factory model, if it has one.
    pub model: Option<String>,
    /// `None`: the key stays as it is.
    pub api_key: Option<String>,
    /// `None`: the effort stays as it is.
    pub effort: Option<String>,
}

/// Puts `choice` into the defaults of `settings` (and so into every profile
/// that follows them, `profile_layers`): the provider, its model (the given
/// one, else its factory model), its key and effort when given. A profile
/// with a provider of its own keeps it. Nothing changes when a part is
/// refused.
pub fn apply_choice(settings: &mut AppSettings, choice: &SetupChoice) -> Result<(), String> {
    let provider_id = choice.provider_id.trim();
    let defaults = crate::profiles::default_values(settings);
    if !defaults
        .post_process_providers
        .iter()
        .any(|provider| provider.id == provider_id)
    {
        return Err(format!("Provider '{provider_id}' not found"));
    }
    if let Some(effort) = &choice.effort {
        if !crate::post_process_effort::LEVELS.contains(&effort.as_str()) {
            return Err(format!("Unknown effort '{effort}'"));
        }
    }
    let model = choice
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let factory = settings::default_model_for_provider(provider_id);
            (!factory.is_empty()).then_some(factory)
        });

    let mut next = settings.clone();
    profile_layers::set_value(
        &mut next,
        &Scope::Defaults,
        "post_process_provider_id",
        serde_json::json!(provider_id),
    )?;
    // The model before the effort: a new model drops an effort it is known
    // not to take (`profile_layers::set_entry`).
    if let Some(model) = model {
        profile_layers::set_entry(
            &mut next,
            &Scope::Defaults,
            "post_process_models",
            provider_id,
            serde_json::json!(model),
        )?;
    }
    if let Some(key) = &choice.api_key {
        profile_layers::set_entry(
            &mut next,
            &Scope::Defaults,
            "post_process_api_keys",
            provider_id,
            serde_json::json!(key.trim()),
        )?;
    }
    if let Some(effort) = &choice.effort {
        profile_layers::set_entry(
            &mut next,
            &Scope::Defaults,
            "post_process_efforts",
            provider_id,
            serde_json::json!(effort),
        )?;
    }
    *settings = next;
    Ok(())
}

/// Makes a finished setup usable (lead's decision, 2026-11-26): defaults
/// that never post-process do so with the instruction shortcut, and with no
/// prompt active the factory prompt (the one the sample runs) becomes
/// active in the defaults.
pub fn make_usable(settings: &mut AppSettings) -> Result<(), String> {
    let layer = with_defaults(settings);
    let mut next = settings.clone();
    if layer.post_process_mode == settings::PostProcessMode::Never {
        profile_layers::set_value(
            &mut next,
            &Scope::Defaults,
            "post_process_mode",
            serde_json::to_value(settings::PostProcessMode::WithInstructionKey)
                .map_err(|error| error.to_string())?,
        )?;
    }
    if crate::prompt_library::requested(&layer).is_empty() {
        let mut prompts = layer.clone();
        with_a_prompt(&mut prompts);
        profile_layers::set_value(
            &mut next,
            &Scope::Defaults,
            "post_process_prompt_order",
            serde_json::to_value(&prompts.post_process_prompt_order)
                .map_err(|error| error.to_string())?,
        )?;
    }
    *settings = next;
    Ok(())
}

/// [`apply_choice`], and when the defaults can post-process after it,
/// [`make_usable`]: what "finishing" a way does. A choice that removes a
/// key leaves the mode and the prompts alone.
pub fn apply_setup_choice(settings: &mut AppSettings, choice: &SetupChoice) -> Result<(), String> {
    let mut next = settings.clone();
    apply_choice(&mut next, choice)?;
    if post_process_ready(&with_defaults(&next)) == Readiness::Ready {
        make_usable(&mut next)?;
    }
    *settings = next;
    Ok(())
}

// ---- Checking a key -----------------------------------------------------------

/// A failure as the setup's commands return it: the reason's name
/// (`post_process_failure`) and the notice's detail, a JSON object that
/// `src/lib/postProcessFailure.ts` reads.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct FailureDto {
    pub reason: String,
    pub detail: Option<String>,
}

impl FailureDto {
    /// `failure` by its reason's name ("unknown" without one) and its
    /// notice's detail.
    pub fn from_failure(failure: &PostProcessFailure) -> Self {
        Self {
            reason: failure.reason.name().unwrap_or("unknown").to_string(),
            detail: failure.notice_detail(),
        }
    }
}

/// What a checked key reaches.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct KeyCheck {
    pub models: Vec<String>,
    /// The model the setup suggests, from [`recommended_model`].
    pub recommended: Option<String>,
}

/// How a model of the preference list is recognized.
enum Pick {
    /// This id.
    Exact(&'static str),
    /// The first id with this in it (the lists name the newest first).
    Containing(&'static str),
}

/// The models the setup suggests per provider, in order: small and fast
/// ones from the providers' own pages and the trial of 2026-11-26.
const PREFERRED: &[(&str, &[Pick])] = &[
    // The trial: 4 of 4 with the user's prompt (strand C's DEFAULT_MODEL).
    (CHATGPT_PROVIDER_ID, &[Pick::Exact("gpt-6.2-sol")]),
    // ai.google.dev/gemini-api/docs/models: "Our fastest, most
    // cost-effective 3.5 model".
    (
        "gemini",
        &[
            Pick::Exact("gemini-3.5-flash-lite"),
            Pick::Containing("flash-lite"),
        ],
    ),
    // The newest Haiku (the decision for OpenRouter with credit: Haiku 5.5).
    ("anthropic", &[Pick::Containing("haiku")]),
    // The trial: 0.7 s in the median on Groq's free plan.
    (
        "groq",
        &[
            Pick::Exact("openai/gpt-oss-120b"),
            Pick::Exact("openai/gpt-oss-20b"),
        ],
    ),
    // huggingface.co/docs/inference-providers: the chat completion page's
    // provider mapping and the index's walkthrough.
    (
        "huggingface",
        &[
            Pick::Exact("openai/gpt-oss-120b"),
            Pick::Exact("deepseek-ai/DeepSeek-V4.1-Flash"),
        ],
    ),
    // docs.ollama.com/api/openai-compatibility: its cloud example.
    (
        OLLAMA_CLOUD_PROVIDER_ID,
        &[Pick::Exact("gemma4:31b"), Pick::Containing("gpt-oss")],
    ),
    // Strand L's PAID_MODEL.
    (
        OPENROUTER_PROVIDER_ID,
        &[Pick::Exact("anthropic/claude-haiku-5.5")],
    ),
];

/// The model to suggest of `models`, the list provider `provider_id` gave:
/// the first of its preference list there, else its factory model if
/// listed. Gemini's ids may come with `models/`, which is left out.
pub fn recommended_model(provider_id: &str, models: &[String]) -> Option<String> {
    let names: Vec<&str> = models
        .iter()
        .map(|model| model.strip_prefix("models/").unwrap_or(model))
        .collect();
    let picks = PREFERRED
        .iter()
        .find(|(id, _)| *id == provider_id)
        .map_or(&[][..], |(_, picks)| *picks);
    for pick in picks {
        let found = match pick {
            Pick::Exact(id) => names.iter().find(|name| *name == id),
            Pick::Containing(part) => names.iter().find(|name| name.contains(part)),
        };
        if let Some(name) = found {
            return Some(name.to_string());
        }
    }
    let factory = settings::default_model_for_provider(provider_id);
    (!factory.is_empty() && names.contains(&factory.as_str())).then_some(factory)
}

/// Providers whose model list anyone may read: the key is checked with a
/// one-token request to the recommended model instead.
const OPEN_MODEL_LISTS: [&str; 2] = [OPENROUTER_PROVIDER_ID, "huggingface"];

/// How the errors of the model list and the one-token request begin
/// (`llm_client::fetch_models`, `send_warmup`); the status in parentheses,
/// then the body.
const LIST_ERROR_PREFIXES: [&str; 2] = ["Model list request failed (", "Warm-up request failed ("];

/// An error of the model list or the one-token request in the form of
/// `llm_client`'s status errors, which `classify_llm_error` reads.
fn as_status_error(error: &str) -> String {
    for prefix in LIST_ERROR_PREFIXES {
        if let Some((status, body)) = error
            .strip_prefix(prefix)
            .and_then(|rest| rest.split_once("): "))
        {
            return format!(
                "{}{}: {}",
                crate::llm_client::STATUS_ERROR_PREFIX,
                status,
                body
            );
        }
    }
    error.to_string()
}

/// The status of a model list's error answer.
fn list_status(error: &str) -> Option<u16> {
    let rest = error.strip_prefix(LIST_ERROR_PREFIXES[0])?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// A failure with `provider`, for `reason`; with the time limit for a
/// timeout.
fn failure_with(
    provider: &PostProcessProvider,
    reason: FailureReason,
    limit: Duration,
) -> FailureDto {
    let seconds = (reason == FailureReason::Timeout).then_some(limit.as_secs());
    FailureDto::from_failure(&PostProcessFailure {
        reason,
        provider: Some((provider.id.clone(), provider.label.clone())),
        model: None,
        seconds,
    })
}

/// Ollama's cloud models by their tags (docs.ollama.com/cloud: the list of
/// cloud models is `https://ollama.com/api/tags`), for when its `/v1`
/// lists none.
async fn ollama_tags(
    provider: &PostProcessProvider,
    api_key: &str,
    limit: Duration,
) -> Result<Vec<String>, String> {
    let base = provider.base_url.trim_end_matches('/');
    let url = format!("{}/api/tags", base.strip_suffix("/v1").unwrap_or(base));
    let client = reqwest::Client::builder()
        .timeout(limit)
        .build()
        .map_err(|e| format!("Failed to fetch models (kind: builder): {e}"))?;
    let mut request = client.get(&url).header(
        reqwest::header::USER_AGENT,
        "DanyWhere/1.0 (+https://github.com/yaccob/Handy)",
    );
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    let response = request.send().await.map_err(|e| {
        let kind = if e.is_connect() {
            "connect"
        } else if e.is_timeout() {
            "timeout"
        } else {
            "request"
        };
        format!("Failed to fetch models (kind: {kind})")
    })?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("Model list request failed ({status}): {text}"));
    }
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| "Failed to parse model list response (kind: decode)".to_string())?;
    Ok(parsed["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|model| model["name"].as_str().or_else(|| model["model"].as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default())
}

/// Checks `api_key` for provider `provider_id` of `settings`' defaults: its
/// model list with the key, and for a list anyone may read a one-token
/// request to the recommended model. Nothing is stored.
pub async fn check_key(
    settings: &AppSettings,
    provider_id: &str,
    api_key: &str,
) -> Result<KeyCheck, FailureDto> {
    let layer = with_defaults(settings);
    let Some(provider) = layer.post_process_provider(provider_id).cloned() else {
        return Err(FailureDto::from_failure(&PostProcessFailure {
            reason: FailureReason::NoProvider,
            provider: None,
            model: None,
            seconds: None,
        }));
    };
    let limit = crate::llm_timeouts::model_list_timeout(&layer);
    if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID {
        let model = settings::APPLE_INTELLIGENCE_DEFAULT_MODEL_ID.to_string();
        return Ok(KeyCheck {
            models: vec![model.clone()],
            recommended: Some(model),
        });
    }
    let api_key = api_key.trim();
    let key_set = !api_key.is_empty();
    if !key_set && needs_key(&provider.id) {
        return Err(failure_with(&provider, FailureReason::NoApiKey, limit));
    }
    let refused = |error: &str| {
        let reason =
            crate::post_process_failure::classify_llm_error(&as_status_error(error), key_set);
        failure_with(&provider, reason, limit)
    };
    let models = match crate::llm_client::fetch_models(&provider, api_key.to_string(), limit).await
    {
        Ok(models) => models,
        Err(error)
            if provider.id == OLLAMA_CLOUD_PROVIDER_ID && list_status(&error) == Some(404) =>
        {
            ollama_tags(&provider, api_key, limit)
                .await
                .map_err(|error| refused(&error))?
        }
        Err(error) => return Err(refused(&error)),
    };
    let recommended = recommended_model(&provider.id, &models);
    if OPEN_MODEL_LISTS.contains(&provider.id.as_str()) {
        if let Some(model) = &recommended {
            crate::llm_client::send_warmup(&provider, api_key, model, limit)
                .await
                .map_err(|error| refused(&error))?;
        }
    }
    Ok(KeyCheck {
        models,
        recommended,
    })
}

// ---- The sample ---------------------------------------------------------------

/// A sample's result.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct SampleResult {
    pub output: String,
    pub model: String,
    pub millis: u64,
}

/// The library's factory prompt ("Improve Transcriptions").
const FACTORY_PROMPT_ID: &str = "default_improve_transcriptions";

/// Activates the factory prompt (or the library's first) in `layer`'s
/// order, for a sample whose defaults run none.
fn with_a_prompt(layer: &mut AppSettings) {
    let mut order = crate::prompt_library::normalized(
        &layer.post_process_prompt_order,
        &layer.post_process_prompts,
    );
    let at = order
        .iter()
        .position(|entry| entry.id == FACTORY_PROMPT_ID)
        .or_else(|| (!order.is_empty()).then_some(0));
    if let Some(at) = at {
        order[at].active = true;
    }
    layer.post_process_prompt_order = order;
}

/// Post-processes `text` with `settings`' defaults, as a dictation would
/// (`actions::process_transcription_with_settings`), also when they never
/// post-process, since the user asked for it; without context. With no
/// prompt active the factory prompt runs, so the sample shows something.
pub async fn run_sample(settings: &AppSettings, text: &str) -> Result<SampleResult, FailureDto> {
    let mut layer = with_defaults(settings);
    layer.post_process_mode = settings::PostProcessMode::Always;
    if crate::prompt_library::requested(&layer).is_empty() {
        with_a_prompt(&mut layer);
    }
    let model = layer
        .active_post_process_provider()
        .and_then(|provider| layer.post_process_models.get(&provider.id))
        .map(|model| model.trim().to_string())
        .unwrap_or_default();
    if text.trim().is_empty() {
        return Err(FailureDto::from_failure(&PostProcessFailure::new(
            &layer,
            FailureReason::other("The sample text is empty"),
        )));
    }
    let started = Instant::now();
    let processed =
        crate::actions::process_transcription_with_settings(&layer, text, None, true, &[], |_| {
            None
        })
        .await;
    let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if let Some(failure) = processed.post_process_failure {
        return Err(FailureDto::from_failure(&failure));
    }
    match processed.post_processed_text {
        Some(output) => Ok(SampleResult {
            output,
            model,
            millis,
        }),
        None => Err(FailureDto::from_failure(&PostProcessFailure::new(
            &layer,
            FailureReason::EmptyReply,
        ))),
    }
}

// ---- Commands -----------------------------------------------------------------

/// What the setup's page and the onboarding screen show: the defaults'
/// readiness, provider, model and way, this computer's memory, the answers.
#[tauri::command]
#[specta::specta]
pub fn get_post_process_setup_status(app: AppHandle) -> SetupStatus {
    let account = crate::post_process_signin::vault::chatgpt_account(&app);
    status(
        &settings::get_settings(&app),
        crate::system_memory::system_memory_bytes(),
        account.as_ref(),
    )
}

/// Stores the setup's answers and whether it was finished or put off.
#[tauri::command]
#[specta::specta]
pub fn set_post_process_setup_answers(
    app: AppHandle,
    answers: Option<SetupAnswers>,
    finished: bool,
    skipped: bool,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.post_process_setup = PostProcessSetupState {
        answers,
        finished,
        skipped,
    };
    settings::write_settings(&app, settings);
    Ok(())
}

/// Puts a way's provider, model, key and effort into the defaults and makes
/// them usable ([`apply_setup_choice`]).
#[tauri::command]
#[specta::specta]
pub fn apply_post_process_choice(app: AppHandle, choice: SetupChoice) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let before = settings.clone();
    apply_setup_choice(&mut settings, &choice)?;
    settings::write_settings(&app, settings.clone());
    crate::profile_effects::apply(&app, &before, &settings);
    Ok(())
}

/// Checks a pasted key ([`check_key`]); stores nothing.
#[tauri::command]
#[specta::specta]
pub async fn check_post_process_key(
    app: AppHandle,
    provider_id: String,
    api_key: String,
) -> Result<KeyCheck, FailureDto> {
    let settings = settings::get_settings(&app);
    check_key(&settings, &provider_id, &api_key).await
}

/// Post-processes a sample text with the defaults ([`run_sample`]).
#[tauri::command]
#[specta::specta]
pub async fn run_post_process_sample(
    app: AppHandle,
    text: String,
) -> Result<SampleResult, FailureDto> {
    let settings = settings::get_settings(&app);
    run_sample(&settings, &text).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{get_default_settings, PostProcessApiFormat, PostProcessMode};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    const GEMINI_KEY: &str = "AIza-test-0123456789";

    /// The default settings, post-processing with `provider_id` and `model`.
    fn with_provider(provider_id: &str, model: &str) -> AppSettings {
        let mut settings = get_default_settings();
        settings.post_process_provider_id = provider_id.to_string();
        settings
            .post_process_models
            .insert(provider_id.to_string(), model.to_string());
        settings
    }

    fn session() -> ChatGptSessionStored {
        ChatGptSessionStored {
            client_id: "app_client_1795300000001".to_string(),
            access_token: "chatgpt-access-secret".to_string(),
            refresh_token: "chatgpt-refresh-secret".to_string(),
            expires_at: 1_795_400_000,
            earliest_refresh_at: Some(1_795_399_700),
            subject: Some("user-subject-secret".to_string()),
            email: Some("someone@example.com".to_string()),
            plan: Some("plus".to_string()),
        }
    }

    #[test]
    fn a_stored_sign_in_from_before_a_field_loads() {
        let stored: ChatGptSessionStored = serde_json::from_value(json!({
            "client_id": "app_1",
            "access_token": "a",
            "refresh_token": "r",
            "expires_at": 1,
        }))
        .expect("loads");
        assert_eq!(stored.earliest_refresh_at, None);
        assert_eq!(stored.subject, None);
        assert_eq!(stored.email, None);
    }

    // ---- Readiness ------------------------------------------------------------

    #[test]
    fn a_provider_with_its_model_and_key_is_ready() {
        let mut settings = with_provider("gemini", "gemini-3.5-flash-lite");
        settings
            .post_process_api_keys
            .insert("gemini".to_string(), GEMINI_KEY.to_string());
        assert_eq!(post_process_ready(&settings), Readiness::Ready);
    }

    #[test]
    fn without_a_provider_or_its_model_it_says_which() {
        let mut gone = with_provider("openai", "gpt-5");
        gone.post_process_provider_id = "gone".to_string();
        assert_eq!(post_process_ready(&gone), Readiness::Provider);
        let blank = with_provider("custom", "  ");
        assert_eq!(post_process_ready(&blank), Readiness::Model);
    }

    #[test]
    fn a_provider_that_needs_a_key_lacks_it_without_one() {
        // OpenAI has a model but no key: not set up (today it failed only
        // at the provider, with 401).
        let settings = with_provider("openai", "gpt-5");
        assert_eq!(post_process_ready(&settings), Readiness::Key);
        let mut blank = with_provider("anthropic", "claude-haiku-5-5");
        blank
            .post_process_api_keys
            .insert("anthropic".to_string(), "   ".to_string());
        assert_eq!(post_process_ready(&blank), Readiness::Key);
    }

    #[test]
    fn chatgpt_needs_the_sign_in_not_a_key() {
        let mut settings = with_provider(CHATGPT_PROVIDER_ID, "gpt-6.2-sol");
        settings.post_process_api_keys.insert(
            CHATGPT_PROVIDER_ID.to_string(),
            "sk-not-a-sign-in".to_string(),
        );
        assert_eq!(post_process_ready(&settings), Readiness::SignIn);
        settings.chatgpt_signed_in = true;
        assert_eq!(post_process_ready(&settings), Readiness::Ready);
    }

    #[test]
    fn a_server_of_ones_own_and_apple_intelligence_need_no_key() {
        assert_eq!(
            post_process_ready(&with_provider("custom", "gemma4:12b")),
            Readiness::Ready
        );
        let mut apple = with_provider(APPLE_INTELLIGENCE_PROVIDER_ID, "Apple Intelligence");
        // Not every platform lists it; the rule doesn't depend on that.
        if apple.active_post_process_provider().is_none() {
            apple.post_process_providers.push(PostProcessProvider {
                id: APPLE_INTELLIGENCE_PROVIDER_ID.to_string(),
                label: "Apple Intelligence".to_string(),
                base_url: "apple-intelligence://local".to_string(),
                allow_base_url_edit: false,
                models_endpoint: None,
                supports_structured_output: true,
                api_format: PostProcessApiFormat::OpenAi,
            });
        }
        assert_eq!(post_process_ready(&apple), Readiness::Ready);
    }

    #[test]
    fn what_is_missing_is_named_in_the_notice() {
        assert_eq!(Readiness::Ready.failure_reason(), None);
        assert_eq!(
            Readiness::Provider.failure_reason(),
            Some(FailureReason::NoProvider)
        );
        assert_eq!(
            Readiness::Model.failure_reason(),
            Some(FailureReason::NoModel)
        );
        assert_eq!(
            Readiness::Key.failure_reason(),
            Some(FailureReason::NoApiKey)
        );
        assert_eq!(
            Readiness::SignIn.failure_reason(),
            Some(FailureReason::NoApiKey)
        );
    }

    // ---- The new providers ---------------------------------------------------

    fn provider_ids(providers: &[PostProcessProvider]) -> Vec<String> {
        providers.iter().map(|p| p.id.clone()).collect()
    }

    const NEW_PROVIDERS: [&str; 4] = ["chatgpt", "gemini", "huggingface", "ollama_cloud"];

    #[test]
    fn the_new_providers_come_after_todays_and_before_custom() {
        let ids = provider_ids(&settings::default_post_process_providers());
        let at = |id: &str| {
            ids.iter()
                .position(|known| known == id)
                .unwrap_or_else(|| panic!("{id} in {ids:?}"))
        };
        let bedrock = at("bedrock_mantle");
        let mut last = bedrock;
        for id in NEW_PROVIDERS {
            assert!(at(id) > last, "{id} after the one before in {ids:?}");
            last = at(id);
        }
        assert_eq!(ids.last().map(String::as_str), Some("custom"));
    }

    #[test]
    fn the_new_providers_are_as_their_documentation_says() {
        let providers = settings::default_post_process_providers();
        let get = |id: &str| {
            providers
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .unwrap_or_else(|| panic!("{id}"))
        };
        let chatgpt = get("chatgpt");
        // The note's "ChatGPT (Abo)" in the main repository's language; the
        // interface names it by its id in the user's language.
        assert_eq!(chatgpt.label, "ChatGPT (subscription)");
        assert_eq!(chatgpt.base_url, "https://api.openai.com/v1");
        assert_eq!(chatgpt.api_format, PostProcessApiFormat::Responses);
        assert!(chatgpt.supports_structured_output);
        let gemini = get("gemini");
        assert_eq!(gemini.label, "Google Gemini");
        assert_eq!(
            gemini.base_url,
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );
        assert_eq!(gemini.api_format, PostProcessApiFormat::OpenAi);
        assert!(gemini.supports_structured_output);
        let huggingface = get("huggingface");
        assert_eq!(huggingface.label, "Hugging Face");
        assert_eq!(huggingface.base_url, "https://router.huggingface.co/v1");
        assert!(!huggingface.supports_structured_output);
        let ollama = get("ollama_cloud");
        assert_eq!(ollama.label, "Ollama Cloud");
        assert_eq!(ollama.base_url, "https://ollama.com/v1");
        assert!(!ollama.supports_structured_output);
        for id in NEW_PROVIDERS {
            assert!(!get(id).allow_base_url_edit, "{id}");
        }
    }

    #[test]
    fn the_new_providers_have_a_factory_model_where_documented() {
        assert_eq!(
            settings::default_model_for_provider("chatgpt"),
            "gpt-6.2-sol"
        );
        assert_eq!(
            settings::default_model_for_provider("gemini"),
            "gemini-3.5-flash-lite"
        );
        assert_eq!(settings::default_model_for_provider("huggingface"), "");
        assert_eq!(settings::default_model_for_provider("ollama_cloud"), "");
    }

    #[test]
    fn the_responses_format_is_stored_by_its_name() {
        assert_eq!(
            serde_json::to_value(PostProcessApiFormat::Responses).unwrap(),
            json!("responses")
        );
        let back: PostProcessApiFormat = serde_json::from_value(json!("responses")).unwrap();
        assert_eq!(back, PostProcessApiFormat::Responses);
    }

    /// Settings as stored before the new providers: without them.
    fn stored_before_the_new_providers() -> AppSettings {
        let mut settings = get_default_settings();
        settings
            .post_process_providers
            .retain(|p| !NEW_PROVIDERS.contains(&p.id.as_str()));
        for id in NEW_PROVIDERS {
            settings.post_process_api_keys.remove(id);
            settings.post_process_models.remove(id);
        }
        settings
    }

    #[test]
    fn older_settings_get_the_new_providers_before_custom() {
        let mut settings = stored_before_the_new_providers();
        assert!(settings::ensure_post_process_defaults(&mut settings));
        let ids = provider_ids(&settings.post_process_providers);
        for id in NEW_PROVIDERS {
            assert!(ids.iter().any(|known| known == id), "{id} in {ids:?}");
            assert_eq!(
                settings.post_process_api_keys.get(id).map(String::as_str),
                Some("")
            );
        }
        assert_eq!(ids.last().map(String::as_str), Some("custom"), "{ids:?}");
        assert_eq!(
            settings
                .post_process_models
                .get("chatgpt")
                .map(String::as_str),
            Some("gpt-6.2-sol")
        );
        assert!(!settings::ensure_post_process_defaults(&mut settings));
    }

    #[test]
    fn the_new_providers_reach_every_profile_and_the_defaults() {
        let mut settings = stored_before_the_new_providers();
        crate::profiles::ensure_profiles(&mut settings);
        let profile = crate::profiles::profile_for_test(&settings, "work", "Work", |_| {});
        settings.profiles.push(profile);
        // The profile in use, so the defaults are repaired as a layer.
        settings.active_profile_id = "work".to_string();
        assert!(settings::ensure_post_process_defaults_everywhere(
            &mut settings
        ));
        let defaults = settings.default_values.as_ref().expect("the defaults");
        for id in NEW_PROVIDERS {
            assert!(
                defaults.post_process_providers.iter().any(|p| p.id == id),
                "{id} in the defaults"
            );
            assert!(
                settings.post_process_providers.iter().any(|p| p.id == id),
                "{id} in the flat values"
            );
        }
    }

    // ---- What is stored -------------------------------------------------------

    #[test]
    fn a_store_from_before_the_setup_loads_with_nothing_set_up() {
        let raw = json!({ "selected_model": "parakeet" });
        let settings: AppSettings = serde_json::from_value(raw).expect("loads");
        assert_eq!(
            settings.post_process_setup,
            PostProcessSetupState::default()
        );
        assert!(!settings.chatgpt_signed_in);
        assert!(settings.chatgpt_host_id.is_none());
        assert!(settings.openrouter_connection.is_none());
    }

    #[test]
    fn the_answers_are_stored_in_snake_case_as_the_questions_name_them() {
        let state = PostProcessSetupState {
            answers: Some(SetupAnswers {
                texts_may_go: Some(TextsMayGo::FreeServices),
                subscription: Some(Subscription::None),
                next_step: Some(NextStep::ThisComputer),
            }),
            finished: false,
            skipped: true,
        };
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(
            value,
            json!({
                "answers": {
                    "texts_may_go": "free_services",
                    "subscription": "none",
                    "next_step": "this_computer",
                },
                "finished": false,
                "skipped": true,
            })
        );
        let partial: SetupAnswers =
            serde_json::from_value(json!({ "texts_may_go": "not_kept" })).unwrap();
        assert_eq!(partial.texts_may_go, Some(TextsMayGo::NotKept));
        assert_eq!(partial.subscription, None);
    }

    #[test]
    fn the_sign_ins_stay_out_of_the_debug_output() {
        let mut settings = get_default_settings();
        settings.chatgpt_host_id = Some(SecretText("urn:uuid:1795300000-host-secret".to_string()));
        let dump = format!("{settings:?}");
        assert!(!dump.contains("host-secret"), "host id");
        assert!(dump.contains(REDACTED));
        // Stored in full.
        let stored = serde_json::to_value(&settings).unwrap();
        assert_eq!(stored["chatgpt_host_id"], "urn:uuid:1795300000-host-secret");
        // The session as stored hides its secrets too.
        let dump = format!("{:?}", session());
        assert!(!dump.contains("chatgpt-access-secret"), "access token");
        assert!(!dump.contains("chatgpt-refresh-secret"), "refresh token");
        assert!(!dump.contains("someone@example.com"), "e-mail");
        assert!(!dump.contains("user-subject-secret"), "subject");
    }

    #[test]
    fn the_setup_and_its_sign_ins_never_travel_with_a_profile() {
        let mut settings = get_default_settings();
        settings.chatgpt_signed_in = true;
        settings.chatgpt_host_id = Some(SecretText("urn:uuid:host-secret".to_string()));
        settings.openrouter_connection = Some(OpenRouterConnection {
            free_tier: true,
            limit_usd: Some(10.0),
            free_models: vec!["nvidia/nemotron-3.5-super-vl:free".to_string()],
            checked_at: 1_795_300_000,
        });
        settings.post_process_setup.finished = true;
        crate::profiles::ensure_profiles(&mut settings);
        let yaml = crate::profile_transfer::build_export(
            &settings,
            crate::profiles::DEFAULT_PROFILE_ID,
            "Standard",
            None,
        )
        .expect("exports without a password: no key travels");
        for absent in [
            "chatgpt-access-secret",
            "chatgpt-refresh-secret",
            "host-secret",
            "chatgpt_signed_in",
            "openrouter_connection",
            "post_process_setup",
            "nemotron",
        ] {
            assert!(!yaml.contains(absent), "{absent} in {yaml}");
        }
        let names = crate::profiles::value_names();
        for field in [
            "post_process_setup",
            "chatgpt_signed_in",
            "chatgpt_host_id",
            "openrouter_connection",
        ] {
            assert!(!names.iter().any(|name| name == field), "{field}");
        }
    }

    // ---- The way and the status -------------------------------------------------

    fn ready(provider_id: &str) -> AppSettings {
        let mut settings = with_provider(provider_id, "model");
        settings
            .post_process_api_keys
            .insert(provider_id.to_string(), "key".to_string());
        settings
    }

    #[test]
    fn the_way_follows_the_provider_and_openrouters_tier() {
        let mut openrouter = ready("openrouter");
        // A key pasted in by hand, without the sign-in.
        assert_eq!(
            current_way(&openrouter, None),
            Some(SetupWay::GuidedKey {
                provider: "openrouter".to_string()
            })
        );
        openrouter.openrouter_connection = Some(OpenRouterConnection {
            free_tier: true,
            limit_usd: Some(10.0),
            free_models: Vec::new(),
            checked_at: 0,
        });
        assert_eq!(
            current_way(&openrouter, None),
            Some(SetupWay::Openrouter {
                free_tier: true,
                limit_usd: Some(10.0)
            })
        );
        if let Some(connection) = openrouter.openrouter_connection.as_mut() {
            connection.free_tier = false;
            connection.limit_usd = None;
        }
        assert_eq!(
            current_way(&openrouter, None),
            Some(SetupWay::Openrouter {
                free_tier: false,
                limit_usd: None
            })
        );

        let mut chatgpt = with_provider(CHATGPT_PROVIDER_ID, "gpt-6.2-sol");
        chatgpt.chatgpt_signed_in = true;
        let account = ChatGptAccount {
            email: Some("someone@example.com".to_string()),
            plan: Some("plus".to_string()),
        };
        assert_eq!(
            current_way(&chatgpt, Some(&account)),
            Some(SetupWay::ChatgptPlan {
                plan: Some("plus".to_string()),
                email: Some("someone@example.com".to_string()),
            })
        );
        assert_eq!(
            current_way(&chatgpt, None),
            Some(SetupWay::ChatgptPlan {
                plan: None,
                email: None
            })
        );
        assert_eq!(
            current_way(&ready("groq"), None),
            Some(SetupWay::GuidedKey {
                provider: "groq".to_string()
            })
        );
        assert_eq!(
            current_way(&with_provider("custom", "gemma4:12b"), None),
            Some(SetupWay::OwnServer)
        );
        // Not ready: no way.
        assert_eq!(current_way(&with_provider("groq", "model"), None), None);
    }

    #[test]
    fn the_status_is_the_defaults() {
        let mut settings = ready("groq");
        settings.post_process_setup.finished = true;
        crate::profiles::ensure_profiles(&mut settings);
        // A profile in use with a provider of its own doesn't change it.
        let profile = crate::profiles::profile_for_test(&settings, "work", "Work", |work| {
            work.post_process_provider_id = "custom".to_string();
        });
        settings.profiles.push(profile);
        settings.active_profile_id = "work".to_string();
        settings.post_process_provider_id = "custom".to_string();
        let status = status(&settings, 17_179_869_184, None);
        assert_eq!(status.readiness, Readiness::Ready);
        assert_eq!(status.provider_id.as_deref(), Some("groq"));
        assert_eq!(status.model.as_deref(), Some("model"));
        assert_eq!(
            status.way,
            Some(SetupWay::GuidedKey {
                provider: "groq".to_string()
            })
        );
        assert_eq!(status.memory_bytes, 17_179_869_184);
        assert!(status.setup.finished);

        let fresh = super::status(&get_default_settings(), 0, None);
        assert_eq!(fresh.provider_id.as_deref(), Some("openai"));
        assert_eq!(fresh.model, None);
        assert_eq!(fresh.readiness, Readiness::Model);
        assert_eq!(fresh.way, None);
    }

    // ---- Applying a way ----------------------------------------------------------

    /// The defaults in use and a profile "Work" that follows them.
    fn with_work_profile() -> AppSettings {
        let mut settings = get_default_settings();
        crate::profiles::ensure_profiles(&mut settings);
        let profile = crate::profiles::profile_for_test(&settings, "work", "Work", |_| {});
        settings.profiles.push(profile);
        settings
    }

    #[test]
    fn a_choice_goes_into_the_defaults_and_the_profiles_that_follow_them() {
        let mut settings = with_work_profile();
        apply_choice(
            &mut settings,
            &SetupChoice {
                provider_id: "gemini".to_string(),
                model: None,
                api_key: Some(format!("  {GEMINI_KEY} ")),
                effort: Some("low".to_string()),
            },
        )
        .expect("applies");
        let defaults = settings.default_values.clone().expect("the defaults");
        assert_eq!(defaults.post_process_provider_id, "gemini");
        // No model given: the provider's factory model.
        assert_eq!(
            defaults
                .post_process_models
                .get("gemini")
                .map(String::as_str),
            Some("gemini-3.5-flash-lite")
        );
        assert_eq!(
            defaults
                .post_process_api_keys
                .get("gemini")
                .map(String::as_str),
            Some(GEMINI_KEY)
        );
        assert_eq!(
            defaults
                .post_process_efforts
                .get("gemini")
                .map(String::as_str),
            Some("low")
        );
        // The defaults are in use: the flat values too.
        assert_eq!(settings.post_process_provider_id, "gemini");
        assert_eq!(post_process_ready(&settings), Readiness::Ready);
        // The profile that follows them.
        let work = &settings.profiles[0].values;
        assert_eq!(work.post_process_provider_id, "gemini");
        assert_eq!(
            work.post_process_api_keys.get("gemini").map(String::as_str),
            Some(GEMINI_KEY)
        );
    }

    #[test]
    fn a_choice_with_a_model_and_without_a_key_keeps_the_key() {
        let mut settings = with_work_profile();
        settings
            .post_process_api_keys
            .insert("groq".to_string(), "gsk-kept".to_string());
        crate::profile_layers::route_flat(&mut settings);
        apply_choice(
            &mut settings,
            &SetupChoice {
                provider_id: "groq".to_string(),
                model: Some("openai/gpt-oss-120b".to_string()),
                api_key: None,
                effort: None,
            },
        )
        .expect("applies");
        let defaults = settings.default_values.clone().expect("the defaults");
        assert_eq!(defaults.post_process_provider_id, "groq");
        assert_eq!(
            defaults.post_process_models.get("groq").map(String::as_str),
            Some("openai/gpt-oss-120b")
        );
        assert_eq!(
            defaults
                .post_process_api_keys
                .get("groq")
                .map(String::as_str),
            Some("gsk-kept")
        );
        assert!(!defaults.post_process_efforts.contains_key("groq"));
    }

    // ---- Finishing a way makes post-processing usable (lead, 2026-11-26) --

    fn groq_with(key: &str) -> SetupChoice {
        SetupChoice {
            provider_id: "groq".to_string(),
            model: Some("openai/gpt-oss-120b".to_string()),
            api_key: Some(key.to_string()),
            effort: None,
        }
    }

    /// The defaults with `mode` and no prompt active (as a new install has).
    fn defaults_with_mode(mode: PostProcessMode) -> AppSettings {
        let mut settings = with_work_profile();
        profile_layers::set_value(
            &mut settings,
            &Scope::Defaults,
            "post_process_mode",
            serde_json::to_value(mode).unwrap(),
        )
        .expect("the mode is set");
        settings
    }

    fn active_prompts(settings: &AppSettings) -> Vec<String> {
        let layer = with_defaults(settings);
        crate::prompt_library::requested(&layer)
            .iter()
            .map(|prompt| prompt.id.clone())
            .collect()
    }

    #[test]
    fn finishing_a_way_makes_defaults_that_never_post_process_use_the_instruction_shortcut() {
        let mut settings = defaults_with_mode(PostProcessMode::Never);
        assert!(
            active_prompts(&settings).is_empty(),
            "a new install's defaults"
        );
        apply_setup_choice(&mut settings, &groq_with("gsk-new")).expect("applies");
        let layer = with_defaults(&settings);
        assert_eq!(layer.post_process_mode, PostProcessMode::WithInstructionKey);
        assert_eq!(
            active_prompts(&settings),
            vec![FACTORY_PROMPT_ID.to_string()]
        );
        assert_eq!(post_process_ready(&layer), Readiness::Ready);
        // The profile that follows the defaults too.
        assert_eq!(
            settings.profiles[0].values.post_process_mode,
            PostProcessMode::WithInstructionKey
        );
    }

    #[test]
    fn finishing_a_way_keeps_a_mode_and_prompts_already_chosen() {
        let mut settings = defaults_with_mode(PostProcessMode::Always);
        // A prompt of the user's own besides the factory one.
        settings.post_process_prompts.push(settings::LLMPrompt {
            id: "prompt_1795300000001".to_string(),
            name: "Mine".to_string(),
            prompt: "Make it short.".to_string(),
        });
        let mut layer = with_defaults(&settings);
        let mut order = crate::prompt_library::normalized(
            &layer.post_process_prompt_order,
            &layer.post_process_prompts,
        );
        let other = order
            .iter()
            .position(|entry| entry.id != FACTORY_PROMPT_ID)
            .expect("the library has another prompt");
        order[other].active = true;
        let chosen = order[other].id.clone();
        layer.post_process_prompt_order = order;
        profile_layers::set_value(
            &mut settings,
            &Scope::Defaults,
            "post_process_prompt_order",
            serde_json::to_value(&layer.post_process_prompt_order).unwrap(),
        )
        .unwrap();
        apply_setup_choice(&mut settings, &groq_with("gsk-new")).expect("applies");
        assert_eq!(
            with_defaults(&settings).post_process_mode,
            PostProcessMode::Always
        );
        assert_eq!(active_prompts(&settings), vec![chosen]);
    }

    #[test]
    fn a_choice_that_removes_the_key_leaves_mode_and_prompts_alone() {
        let mut settings = defaults_with_mode(PostProcessMode::Never);
        apply_setup_choice(&mut settings, &groq_with("")).expect("applies");
        let layer = with_defaults(&settings);
        assert_eq!(layer.post_process_mode, PostProcessMode::Never);
        assert!(active_prompts(&settings).is_empty());
        assert_eq!(post_process_ready(&layer), Readiness::Key);
    }

    #[test]
    fn making_usable_twice_changes_nothing_more() {
        let mut settings = defaults_with_mode(PostProcessMode::Never);
        make_usable(&mut settings).unwrap();
        let once = settings.clone();
        make_usable(&mut settings).unwrap();
        assert_eq!(
            serde_json::to_value(&settings).unwrap(),
            serde_json::to_value(&once).unwrap()
        );
        assert_eq!(
            with_defaults(&settings).post_process_mode,
            PostProcessMode::WithInstructionKey
        );
    }

    #[test]
    fn a_choice_of_an_unknown_provider_or_effort_is_refused() {
        let mut settings = with_work_profile();
        let before = settings.default_values.clone();
        let unknown = SetupChoice {
            provider_id: "nowhere".to_string(),
            model: Some("m".to_string()),
            api_key: None,
            effort: None,
        };
        assert!(apply_choice(&mut settings, &unknown).is_err());
        let bad_effort = SetupChoice {
            provider_id: "groq".to_string(),
            model: Some("m".to_string()),
            api_key: None,
            effort: Some("enormous".to_string()),
        };
        assert!(apply_choice(&mut settings, &bad_effort).is_err());
        assert_eq!(settings.default_values, before, "nothing changed");
    }

    // ---- The recommended model ---------------------------------------------------

    fn names(models: &[&str]) -> Vec<String> {
        models.iter().map(|m| m.to_string()).collect()
    }

    #[test]
    fn the_recommended_model_comes_from_the_preference_list() {
        assert_eq!(
            recommended_model(
                "groq",
                &names(&[
                    "llama-3.1-8b-instant",
                    "openai/gpt-oss-20b",
                    "openai/gpt-oss-120b"
                ])
            ),
            Some("openai/gpt-oss-120b".to_string())
        );
        // Gemini's list may name them with "models/".
        assert_eq!(
            recommended_model(
                "gemini",
                &names(&["models/gemini-3.8-flash", "models/gemini-3.5-flash-lite"])
            ),
            Some("gemini-3.5-flash-lite".to_string())
        );
        // The newest Haiku, which Anthropic's list names first.
        assert_eq!(
            recommended_model(
                "anthropic",
                &names(&["claude-opus-5-5", "claude-haiku-5-5", "claude-haiku-4-5"])
            ),
            Some("claude-haiku-5-5".to_string())
        );
        assert_eq!(
            recommended_model("ollama_cloud", &names(&["qwen3:480b", "gemma4:31b"])),
            Some("gemma4:31b".to_string())
        );
        assert_eq!(
            recommended_model(
                "huggingface",
                &names(&["zai-org/GLM-5.3", "openai/gpt-oss-120b"])
            ),
            Some("openai/gpt-oss-120b".to_string())
        );
        // Nothing preferred in the list: none.
        assert_eq!(
            recommended_model("groq", &names(&["whisper-large-v3"])),
            None
        );
        assert_eq!(recommended_model("zai", &names(&["glm-5"])), None);
    }

    // ---- Checking a key against a server -------------------------------------------

    /// Answers one request per entry of `responses`, in turn, and hands over
    /// each request's first line and body.
    fn serve_in_turn(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (String, mpsc::Receiver<(String, String)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for (status, body) in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut request = Vec::new();
                let mut buffer = [0_u8; 8192];
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
                            let first = text.lines().next().unwrap_or_default().to_string();
                            let _ = tx.send((first, text[end + 4..].to_string()));
                            break;
                        }
                    }
                }
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://{address}"), rx)
    }

    const LONG: Duration = Duration::from_secs(10);

    /// The default settings with provider `id` at `base_url`.
    fn at(id: &str, base_url: &str) -> AppSettings {
        let mut settings = get_default_settings();
        let provider = settings
            .post_process_provider_mut(id)
            .unwrap_or_else(|| panic!("{id}"));
        provider.base_url = base_url.to_string();
        settings
    }

    fn check(settings: &AppSettings, id: &str, key: &str) -> Result<KeyCheck, FailureDto> {
        tauri::async_runtime::block_on(check_key(settings, id, key))
    }

    #[test]
    fn a_key_is_checked_with_the_providers_model_list() {
        let (url, requests) = serve_in_turn(vec![(
            "200 OK",
            r#"{"data":[{"id":"llama-3.1-8b-instant"},{"id":"openai/gpt-oss-120b"}]}"#,
        )]);
        let result = check(&at("groq", &url), "groq", "gsk-test");
        assert_eq!(
            result,
            Ok(KeyCheck {
                models: names(&["llama-3.1-8b-instant", "openai/gpt-oss-120b"]),
                recommended: Some("openai/gpt-oss-120b".to_string()),
            })
        );
        let (first, _) = requests.recv_timeout(LONG).expect("the model list");
        assert!(first.starts_with("GET /models"), "{first}");
    }

    #[test]
    fn a_refused_key_says_so() {
        let (url, _requests) = serve_in_turn(vec![(
            "401 Unauthorized",
            r#"{"error":{"message":"Invalid API Key","type":"invalid_request_error","code":"invalid_api_key"}}"#,
        )]);
        let result = check(&at("groq", &url), "groq", "gsk-wrong");
        let failure = result.expect_err("refused");
        assert_eq!(failure.reason, "key-refused");
        let detail: serde_json::Value =
            serde_json::from_str(failure.detail.as_deref().expect("a detail")).unwrap();
        assert_eq!(detail["reason"], "key-refused");
        assert_eq!(detail["providerId"], "groq");
        assert_eq!(detail["provider"], "Groq");
    }

    #[test]
    fn a_key_whose_model_list_is_open_to_all_is_tried_with_one_token() {
        // Hugging Face's router lists its models to anyone: a request for
        // one token with the recommended model checks the key.
        let (url, requests) = serve_in_turn(vec![
            ("200 OK", r#"{"data":[{"id":"openai/gpt-oss-120b"}]}"#),
            (
                "401 Unauthorized",
                r#"{"error":"Invalid credentials in Authorization header"}"#,
            ),
        ]);
        let result = check(&at("huggingface", &url), "huggingface", "hf_wrong");
        assert_eq!(result.expect_err("refused").reason, "key-refused");
        let _ = requests.recv_timeout(LONG).expect("the model list");
        let (first, body) = requests.recv_timeout(LONG).expect("the one-token request");
        assert!(first.starts_with("POST /chat/completions"), "{first}");
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["model"], "openai/gpt-oss-120b");
        assert_eq!(body["max_tokens"], 1);
    }

    #[test]
    fn ollama_cloud_lists_its_models_by_their_tags_when_v1_has_none() {
        let (url, requests) = serve_in_turn(vec![
            ("404 Not Found", "404 page not found"),
            (
                "200 OK",
                r#"{"models":[{"name":"qwen3:480b","model":"qwen3:480b"},{"name":"gemma4:31b","model":"gemma4:31b"}]}"#,
            ),
        ]);
        let base = format!("{url}/v1");
        let result = check(&at("ollama_cloud", &base), "ollama_cloud", "ollama-test");
        assert_eq!(
            result,
            Ok(KeyCheck {
                models: names(&["qwen3:480b", "gemma4:31b"]),
                recommended: Some("gemma4:31b".to_string()),
            })
        );
        let (first, _) = requests.recv_timeout(LONG).unwrap();
        assert!(first.starts_with("GET /v1/models"), "{first}");
        let (second, _) = requests.recv_timeout(LONG).unwrap();
        assert!(second.starts_with("GET /api/tags"), "{second}");
    }

    #[test]
    fn an_empty_key_is_refused_without_a_request() {
        let (url, requests) = serve_in_turn(vec![("200 OK", r#"{"data":[]}"#)]);
        let result = check(&at("groq", &url), "groq", "  ");
        assert_eq!(result.expect_err("no key").reason, "no-api-key");
        assert!(requests.recv_timeout(Duration::from_millis(300)).is_err());
        let unknown = check(&get_default_settings(), "nowhere", "key");
        assert_eq!(unknown.expect_err("no provider").reason, "no-provider");
    }

    // ---- The sample -------------------------------------------------------------

    const POLISHED: &str =
        r#"{"choices":[{"message":{"content":"polished"},"finish_reason":"stop"}]}"#;

    /// Defaults (in use) that post-process with the custom server at
    /// `base_url` and model "model", set to never post-process, no prompt
    /// active.
    fn sample_settings(base_url: &str) -> AppSettings {
        let mut settings = at("custom", base_url);
        settings.post_process_provider_id = "custom".to_string();
        settings
            .post_process_models
            .insert("custom".to_string(), "model".to_string());
        settings.post_process_mode = PostProcessMode::Never;
        crate::profiles::ensure_profiles(&mut settings);
        settings
    }

    #[test]
    fn the_sample_runs_with_the_defaults_even_when_they_never_post_process() {
        let (url, requests) = serve_in_turn(vec![("200 OK", POLISHED)]);
        let result =
            tauri::async_runtime::block_on(run_sample(&sample_settings(&url), "hello there"));
        let sample = result.expect("a result");
        assert_eq!(sample.output, "polished");
        assert_eq!(sample.model, "model");
        let (first, body) = requests.recv_timeout(LONG).expect("the request");
        assert!(first.starts_with("POST /chat/completions"), "{first}");
        // No prompt was active: the factory prompt asks.
        assert!(body.contains("hello there"), "{body}");
        assert!(body.contains("Clean up the transcript"), "{body}");
    }

    #[test]
    fn a_failed_sample_names_its_reason() {
        let (url, _requests) = serve_in_turn(vec![(
            "404 Not Found",
            r#"{"error":{"message":"model 'model' not found","type":"not_found_error","param":null,"code":null}}"#,
        )]);
        let result = tauri::async_runtime::block_on(run_sample(&sample_settings(&url), "hello"));
        let failure = result.expect_err("fails");
        assert_eq!(failure.reason, "model-not-found");
        let detail: serde_json::Value =
            serde_json::from_str(failure.detail.as_deref().expect("a detail")).unwrap();
        assert_eq!(detail["model"], "model");
        // Not set up: no request.
        let not_set_up = with_provider("groq", "model");
        let result = tauri::async_runtime::block_on(run_sample(&not_set_up, "hello"));
        assert_eq!(result.expect_err("not set up").reason, "no-api-key");
    }

    #[test]
    fn a_failure_carries_its_reason_and_the_notices_detail() {
        let failure = PostProcessFailure::new(&ready("groq"), FailureReason::RateLimited);
        let dto = FailureDto::from_failure(&failure);
        assert_eq!(dto.reason, "rate-limited");
        assert_eq!(dto.detail, failure.notice_detail());
        let unknown = FailureDto::from_failure(&PostProcessFailure::unknown());
        assert_eq!(unknown.reason, "unknown");
        assert_eq!(unknown.detail, None);
    }
}
