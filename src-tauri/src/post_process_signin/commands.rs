//! DanyWhere: the browser sign-in as the app runs it (sidecar note
//! `2026-11-26_2250_nachbearbeitung-einstieg-umsetzung`, sections 2, 4.1, 4.2
//! and 4.5): one sign-in at a time, waiting for the provider's callback in a
//! thread of its own for at most [`SIGN_IN_TIMEOUT`], then exchanging the
//! code at once (OpenRouter's codes last ten minutes) and storing what it
//! gives.
//!
//! [`SignIns`] holds the sign-in in progress. What it needs of the app, the
//! browser, the settings and the event, comes through [`SignInHost`], so a
//! whole sign-in runs in tests against fake servers, with the test playing
//! the browser.

use super::chatgpt::{self, ChatGptError, ChatGptFlow, ChatGptSession};
use super::loopback::{self, PendingSignIn, SignInError};
use super::openrouter::{self, KeyInfo, OpenRouterFlow};
use super::session::SessionStore;
use super::vault::{self, Vault};
use super::{SignInCode, SignInFlow};
use crate::post_process_failure::{FailureReason, PostProcessFailure};
use crate::post_process_setup::{
    FailureDto, OpenRouterConnection, SecretText, SetupChoice, CHATGPT_PROVIDER_ID,
    OPENROUTER_PROVIDER_ID,
};
use crate::profile_layers::{self, Scope};
use crate::settings::AppSettings;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use specta::Type;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;

/// How long a sign-in waits for the callback (the newcomer in the trial of
/// 2026-11-26 needed 17 minutes).
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// `error` of [`PostProcessSignInFinished`] when the user cancelled.
pub const ERROR_CANCELLED: &str = "cancelled";
/// … when no callback came within [`SIGN_IN_TIMEOUT`].
pub const ERROR_TIMED_OUT: &str = "timed_out";
/// … when the user refused in the browser (`error=` in the callback; for
/// ChatGPT `access_denied`: plan usage not allowed).
pub const ERROR_DENIED: &str = "denied";
/// … when ChatGPT signed in but did not grant plan usage.
pub const ERROR_NOT_ELIGIBLE: &str = "not_eligible";

/// The provider a sign-in is for.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "lowercase")]
pub enum SignInProvider {
    Openrouter,
    Chatgpt,
}

/// The two short texts the browser shows after the callback, in the app's
/// language.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct CallbackPage {
    pub success: String,
    pub failure: String,
}

/// A started sign-in: the page the browser was sent to.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct SignInStarted {
    pub auth_url: String,
}

/// How a sign-in ended. `error`: [`ERROR_CANCELLED`], [`ERROR_TIMED_OUT`],
/// [`ERROR_DENIED`], [`ERROR_NOT_ELIGIBLE`], or the error's text (the
/// `llm_client` forms, "sign-in-expired: …", …).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct PostProcessSignInFinished {
    pub provider: SignInProvider,
    pub ok: bool,
    pub error: Option<String>,
    /// OpenRouter after a sign-in: whether the account is on the free tier.
    pub free_tier: Option<bool>,
}

/// Where the sign-in's requests go: the real services in the app, fake ones
/// in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// OpenRouter's site ([`openrouter::DEFAULT_BASE`]): the code exchange,
    /// the key info and the model list.
    pub openrouter_base: String,
    /// ChatGPT's token endpoint ([`chatgpt::TOKEN`]).
    pub chatgpt_token_url: String,
}

impl Endpoints {
    /// The real services.
    pub fn real() -> Self {
        Endpoints {
            openrouter_base: openrouter::DEFAULT_BASE.to_string(),
            chatgpt_token_url: chatgpt::TOKEN.to_string(),
        }
    }
}

/// What OpenRouter says about a key, and the models to ask with it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRouterState {
    pub info: KeyInfo,
    /// The free models to try, in order (at most
    /// [`openrouter::MAX_FALLBACK_MODELS`]).
    pub free_models: Vec<String>,
    /// The model to ask: on the free tier the first free model, else
    /// [`openrouter::PAID_MODEL`].
    pub model: String,
}

/// A finished OpenRouter sign-in: the new key and what it reaches. Its
/// `Debug` hides the key.
pub struct OpenRouterSignedIn {
    pub key: String,
    pub state: OpenRouterState,
}

impl std::fmt::Debug for OpenRouterSignedIn {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenRouterSignedIn")
            .field("key", &"<hidden>")
            .field("state", &self.state)
            .finish()
    }
}

/// What a sign-in needs of the app.
pub trait SignInHost: Send + Sync + 'static {
    /// Opens `url` in the browser.
    fn open_url(&self, url: &str) -> Result<(), String>;
    /// ChatGPT: the client id issued at an earlier registration, if any,
    /// and this installation's host id, made and saved on first use.
    fn chatgpt_identity(&self) -> Result<(Option<String>, String), String>;
    /// ChatGPT: saves the client id a first registration was issued, before
    /// the code is exchanged (the registration stands even if the exchange
    /// fails).
    fn save_chatgpt_client_id(&self, client_id: &str) -> Result<(), String>;
    /// ChatGPT: stores the session and sets the plan up for post-processing.
    fn store_chatgpt(&self, session: ChatGptSession) -> Result<(), String>;
    /// OpenRouter: stores the key and what it reaches and sets OpenRouter up
    /// for post-processing.
    fn store_openrouter(&self, signed_in: OpenRouterSignedIn) -> Result<(), String>;
    /// Tells the frontend how the sign-in ended.
    fn finished(&self, event: PostProcessSignInFinished);
}

/// The sign-in in progress, at most one, kept in Tauri's state.
#[derive(Default, Clone)]
pub struct SignIns {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    current: Mutex<Option<Attempt>>,
    next_id: AtomicU64,
}

struct Attempt {
    id: u64,
    provider: SignInProvider,
    auth_url: String,
    cancel: Arc<AtomicBool>,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Option<Attempt>> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_current(&self, id: u64) -> bool {
        self.lock().as_ref().is_some_and(|attempt| attempt.id == id)
    }

    /// Ends attempt `id` if it is still the current one; returns whether it
    /// was.
    fn finish(&self, id: u64) -> bool {
        let mut current = self.lock();
        if current.as_ref().is_some_and(|attempt| attempt.id == id) {
            *current = None;
            true
        } else {
            false
        }
    }
}

/// The provider's side of one attempt, kept for the exchange.
enum Flow {
    OpenRouter(OpenRouterFlow),
    ChatGpt(ChatGptFlow),
}

impl Flow {
    fn sign_in_flow(&self) -> &dyn SignInFlow {
        match self {
            Flow::OpenRouter(flow) => flow,
            Flow::ChatGpt(flow) => flow,
        }
    }
}

impl SignIns {
    /// Starts signing in with `provider`: binds the loopback listener, opens
    /// the provider's page in the browser and waits for the callback in a
    /// thread of its own for at most `timeout`. A sign-in already in
    /// progress ends without an event. Returns the page's URL.
    pub fn start(
        &self,
        provider: SignInProvider,
        page: CallbackPage,
        host: Arc<dyn SignInHost>,
        endpoints: Endpoints,
        timeout: Duration,
    ) -> Result<String, String> {
        let flow = match provider {
            SignInProvider::Openrouter => Flow::OpenRouter(OpenRouterFlow::danywhere()),
            SignInProvider::Chatgpt => {
                let (client_id, host_id) = host.chatgpt_identity()?;
                Flow::ChatGpt(ChatGptFlow::new(client_id, host_id)?)
            }
        };
        let pending = PendingSignIn::start(
            flow.sign_in_flow(),
            loopback::CallbackPage {
                success: page.success,
                failure: page.failure,
            },
        )?;
        let auth_url = pending.auth_url().to_string();
        log::debug!(
            "The {provider:?} sign-in calls back on {}",
            pending.redirect_uri()
        );
        host.open_url(&auth_url)
            .map_err(|error| format!("Cannot open the sign-in page in the browser: {error}"))?;

        let cancel = Arc::new(AtomicBool::new(false));
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let replaced = self.inner.lock().replace(Attempt {
            id,
            provider,
            auth_url: auth_url.clone(),
            cancel: cancel.clone(),
        });
        if let Some(old) = replaced {
            log::info!(
                "A new {provider:?} sign-in replaces the {:?} one in progress",
                old.provider
            );
            old.cancel.store(true, Ordering::SeqCst);
        }
        log::info!("Waiting for the {provider:?} sign-in in the browser");

        let inner = self.inner.clone();
        let spawned = std::thread::Builder::new()
            .name("post-process-sign-in".to_string())
            .spawn(move || {
                let event = match pending.wait(timeout, &cancel) {
                    // Replaced by a newer sign-in, which reports for itself.
                    Err(SignInError::Cancelled) if !inner.is_current(id) => return,
                    Err(error) => failed(provider, wait_error_text(provider, &error)),
                    Ok(code) => tauri::async_runtime::block_on(complete(
                        flow,
                        code,
                        host.as_ref(),
                        &endpoints,
                    )),
                };
                if inner.finish(id) {
                    host.finished(event);
                } else {
                    log::info!("A replaced {provider:?} sign-in ended: {:?}", event.error);
                }
            });
        if let Err(error) = spawned {
            self.inner.finish(id);
            return Err(format!("Cannot wait for the sign-in: {error}"));
        }
        Ok(auth_url)
    }

    /// Opens the page of the sign-in in progress again; returns its URL.
    pub fn reopen(&self, host: &dyn SignInHost) -> Result<String, String> {
        let auth_url = self
            .inner
            .lock()
            .as_ref()
            .map(|attempt| attempt.auth_url.clone())
            .ok_or_else(|| "No sign-in is in progress".to_string())?;
        host.open_url(&auth_url)
            .map_err(|error| format!("Cannot open the sign-in page in the browser: {error}"))?;
        Ok(auth_url)
    }

    /// Stops waiting for the sign-in in progress; it ends as
    /// [`ERROR_CANCELLED`].
    pub fn cancel(&self) {
        if let Some(attempt) = self.inner.lock().as_ref() {
            log::info!("Cancelling the {:?} sign-in", attempt.provider);
            attempt.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// The provider of the sign-in in progress.
    pub fn in_progress(&self) -> Option<SignInProvider> {
        self.inner.lock().as_ref().map(|attempt| attempt.provider)
    }
}

fn failed(provider: SignInProvider, error: String) -> PostProcessSignInFinished {
    PostProcessSignInFinished {
        provider,
        ok: false,
        error: Some(error),
        free_tier: None,
    }
}

/// The event's `error` for a wait that brought no code.
fn wait_error_text(provider: SignInProvider, error: &SignInError) -> String {
    match error {
        SignInError::Cancelled => ERROR_CANCELLED.to_string(),
        SignInError::TimedOut => ERROR_TIMED_OUT.to_string(),
        SignInError::Denied(reason) => {
            log::info!("The {provider:?} sign-in was refused in the browser: {reason}");
            ERROR_DENIED.to_string()
        }
        other => {
            log::warn!("The {provider:?} sign-in failed: {other}");
            other.to_string()
        }
    }
}

/// Exchanges the code and stores what it gives.
async fn complete(
    flow: Flow,
    code: SignInCode,
    host: &dyn SignInHost,
    endpoints: &Endpoints,
) -> PostProcessSignInFinished {
    let (provider, outcome) = match flow {
        Flow::OpenRouter(_) => (
            SignInProvider::Openrouter,
            complete_openrouter(&code, host, endpoints).await.map(Some),
        ),
        Flow::ChatGpt(flow) => (
            SignInProvider::Chatgpt,
            complete_chatgpt(&flow, &code, host, endpoints)
                .await
                .map(|()| None),
        ),
    };
    match outcome {
        Ok(free_tier) => {
            log::info!("The {provider:?} sign-in is done");
            PostProcessSignInFinished {
                provider,
                ok: true,
                error: None,
                free_tier,
            }
        }
        Err(error) => {
            log::warn!("The {provider:?} sign-in failed: {error}");
            failed(provider, error)
        }
    }
}

/// OpenRouter: the key for the code, what it reaches; returns whether the
/// account is on the free tier.
async fn complete_openrouter(
    code: &SignInCode,
    host: &dyn SignInHost,
    endpoints: &Endpoints,
) -> Result<bool, String> {
    let client = token_client()?;
    let key = openrouter::exchange_code(&client, &endpoints.openrouter_base, code).await?;
    let state = openrouter_state(&client, &endpoints.openrouter_base, &key).await?;
    let free_tier = state.info.free_tier;
    host.store_openrouter(OpenRouterSignedIn { key, state })?;
    Ok(free_tier)
}

/// ChatGPT: saves a newly issued client id first, then the session for the
/// code.
async fn complete_chatgpt(
    flow: &ChatGptFlow,
    code: &SignInCode,
    host: &dyn SignInHost,
    endpoints: &Endpoints,
) -> Result<(), String> {
    let usable = |id: &&str| !id.is_empty() && *id != chatgpt::DYNAMIC_CLIENT;
    let registered_before = flow.client_id.as_deref().filter(usable).is_some();
    if let Some(issued) = code.client_id.as_deref().filter(usable) {
        if !registered_before {
            if let Err(error) = host.save_chatgpt_client_id(issued) {
                log::error!("Cannot save ChatGPT's issued client id: {error}");
            }
        }
    }
    let client = token_client()?;
    let session = chatgpt::exchange_code(&client, &endpoints.chatgpt_token_url, flow, code)
        .await
        .map_err(|error| match error {
            ChatGptError::NotEligible => ERROR_NOT_ELIGIBLE.to_string(),
            other => other.to_string(),
        })?;
    host.store_chatgpt(session)
}

/// The client for requests that carry a code or a token: no redirects, so a
/// 307/308 can't send them to another address.
pub fn token_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Failed to build the sign-in HTTP client: {error}"))
}

/// [`token_client`] with `timeout` for connecting and for each request,
/// for what happens on the side: OpenRouter's tier asked again
/// (`openrouter_tier`), the revocation when signing out of ChatGPT.
pub fn short_token_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(timeout)
        .timeout(timeout)
        .build()
        .map_err(|error| format!("Failed to build the sign-in HTTP client: {error}"))
}

/// How long each request of the revocation may take when signing out of
/// ChatGPT (the discovery document, then the revocation).
pub const REVOKE_TIMEOUT: Duration = Duration::from_secs(4);

/// Signing out of ChatGPT: ends the stored session's renewable session at
/// OpenAI first ("Accounts and sessions", "End the renewable session"; best
/// effort, each request within `timeout`), through the `revocation_endpoint`
/// of the discovery document at `discovery_url`. Returns whether OpenAI
/// confirmed it; without a stored session nothing is sent. The caller then
/// forgets the tokens here ([`end_chatgpt_sign_in`]) either way.
pub async fn revoke_chatgpt_session(vault: &Vault, discovery_url: &str, timeout: Duration) -> bool {
    let Some(session) = vault.load().chatgpt() else {
        return false;
    };
    let client = match short_token_client(timeout) {
        Ok(client) => client,
        Err(error) => {
            log::warn!("Cannot end the ChatGPT sign-in at OpenAI: {error}");
            return false;
        }
    };
    match chatgpt::revoke(&client, discovery_url, &session).await {
        Ok(()) => {
            log::info!("OpenAI ended the ChatGPT sign-in's renewable session");
            true
        }
        Err(error) => {
            // "Errors and recovery": clear the tokens here all the same; the
            // user can disconnect DanyWhere in ChatGPT's settings.
            log::warn!(
                "OpenAI did not confirm the end of the ChatGPT sign-in ({error}); it ends here only"
            );
            false
        }
    }
}

/// The model to ask OpenRouter: the first free model on the free tier,
/// else [`openrouter::PAID_MODEL`].
pub fn openrouter_model(free_tier: bool, free_models: &[String]) -> String {
    if free_tier {
        free_models.first().cloned().unwrap_or_default()
    } else {
        openrouter::PAID_MODEL.to_string()
    }
}

/// What OpenRouter says about `key` and the free models to try: from the
/// model list, or OpenRouter's preferred free models when the list can't be
/// read.
pub async fn openrouter_state(
    client: &reqwest::Client,
    base: &str,
    key: &str,
) -> Result<OpenRouterState, String> {
    let info = openrouter::key_info(client, base, key).await?;
    let free_models = match openrouter::list_models(client, base).await {
        Ok(models) => {
            openrouter::choose_free_models(&models, true, openrouter::MAX_FALLBACK_MODELS)
        }
        Err(error) => {
            log::warn!("OpenRouter's model list is unavailable ({error}); trying the preferred free models");
            openrouter::FREE_MODEL_PREFERENCE
                .iter()
                .take(openrouter::MAX_FALLBACK_MODELS)
                .map(|model| model.to_string())
                .collect()
        }
    };
    let model = openrouter_model(info.free_tier, &free_models);
    Ok(OpenRouterState {
        info,
        free_models,
        model,
    })
}

// ---- The app's side: the settings and the sign-in's file ----------------------

/// After a ChatGPT sign-in: signed in, and ChatGPT's plan in the defaults
/// with its model and effort `low`, usable at once
/// (`post_process_setup::make_usable`).
pub fn apply_chatgpt_sign_in(settings: &mut AppSettings) -> Result<(), String> {
    let mut next = settings.clone();
    next.chatgpt_signed_in = true;
    crate::post_process_setup::apply_setup_choice(
        &mut next,
        &SetupChoice {
            provider_id: CHATGPT_PROVIDER_ID.to_string(),
            model: Some(chatgpt::DEFAULT_MODEL.to_string()),
            api_key: None,
            effort: Some(chatgpt::DEFAULT_EFFORT.to_string()),
        },
    )?;
    *settings = next;
    Ok(())
}

/// Stores a ChatGPT sign-in: the session and its client id in `vault`, the
/// rest in `settings` ([`apply_chatgpt_sign_in`]); no token in the settings.
pub fn store_chatgpt_sign_in(
    vault: &Vault,
    settings: &mut AppSettings,
    session: &ChatGptSession,
) -> Result<(), String> {
    vault.update(|secrets| {
        secrets.chatgpt_client_id = Some(session.client_id.clone());
        secrets.chatgpt_session = Some(session.into());
    })?;
    apply_chatgpt_sign_in(settings)
}

/// ChatGPT's saved client id (from `vault`) and this installation's host
/// id, made once and kept in `settings`; the flag says whether it was made
/// now (the settings need saving).
pub fn chatgpt_identity_from(
    vault: &Vault,
    settings: &mut AppSettings,
) -> Result<(Option<String>, String, bool), String> {
    let client_id = vault
        .load()
        .chatgpt_client_id
        .filter(|id| !id.is_empty() && id != chatgpt::DYNAMIC_CLIENT);
    if let Some(host_id) = settings
        .chatgpt_host_id
        .as_ref()
        .filter(|host_id| !host_id.0.is_empty())
    {
        return Ok((client_id, host_id.0.clone(), false));
    }
    let host_id = chatgpt::new_host_id()?;
    settings.chatgpt_host_id = Some(SecretText(host_id.clone()));
    Ok((client_id, host_id, true))
}

/// The ChatGPT sign-in ended (dead refresh token, or signed out): the
/// tokens go, the client id and the host id stay.
pub fn end_chatgpt_sign_in(vault: &Vault, settings: &mut AppSettings) -> Result<(), String> {
    vault.update(|secrets| secrets.chatgpt_session = None)?;
    settings.chatgpt_signed_in = false;
    Ok(())
}

fn connection_of(state: &OpenRouterState, now: i64) -> OpenRouterConnection {
    OpenRouterConnection {
        free_tier: state.info.free_tier,
        limit_usd: state.info.limit_usd,
        free_models: state.free_models.clone(),
        checked_at: now,
    }
}

/// After an OpenRouter sign-in at `now` (Unix seconds): the key, the model
/// and OpenRouter in the defaults, what the key reaches in
/// `openrouter_connection`, usable at once.
pub fn apply_openrouter_sign_in(
    settings: &mut AppSettings,
    signed_in: &OpenRouterSignedIn,
    now: i64,
) -> Result<(), String> {
    let mut next = settings.clone();
    next.openrouter_connection = Some(connection_of(&signed_in.state, now));
    crate::post_process_setup::apply_setup_choice(
        &mut next,
        &SetupChoice {
            provider_id: OPENROUTER_PROVIDER_ID.to_string(),
            model: Some(signed_in.state.model.clone()).filter(|model| !model.is_empty()),
            api_key: Some(signed_in.key.clone()),
            effort: None,
        },
    )?;
    *settings = next;
    Ok(())
}

/// After OpenRouter was asked again at `now`: the connection anew, and the
/// defaults' OpenRouter model follows the tier (the first free model, or the
/// paid one after topping up) unless the user chose another one.
pub fn apply_openrouter_refresh(
    settings: &mut AppSettings,
    state: &OpenRouterState,
    now: i64,
) -> Result<OpenRouterConnection, String> {
    let mut next = settings.clone();
    let connection = connection_of(state, now);
    let before = next.openrouter_connection.replace(connection.clone());
    let current = crate::post_process_setup::with_defaults(&next)
        .post_process_models
        .get(OPENROUTER_PROVIDER_ID)
        .map(|model| model.trim().to_string())
        .unwrap_or_default();
    // Ours: none yet, the paid one, or one of the free ones the sign-in chose.
    let ours = current.is_empty()
        || current == openrouter::PAID_MODEL
        || before.is_some_and(|before| before.free_models.contains(&current))
        || state.free_models.contains(&current);
    if ours && !state.model.is_empty() && current != state.model {
        profile_layers::set_entry(
            &mut next,
            &Scope::Defaults,
            "post_process_models",
            OPENROUTER_PROVIDER_ID,
            Value::String(state.model.clone()),
        )?;
    }
    *settings = next;
    Ok(connection)
}

/// Forgets OpenRouter's sign-in: the defaults' key and the connection.
pub fn end_openrouter_sign_in(settings: &mut AppSettings) -> Result<(), String> {
    let mut next = settings.clone();
    next.openrouter_connection = None;
    profile_layers::set_entry(
        &mut next,
        &Scope::Defaults,
        "post_process_api_keys",
        OPENROUTER_PROVIDER_ID,
        Value::String(String::new()),
    )?;
    *settings = next;
    Ok(())
}

/// The fields an OpenRouter request gets from the sign-in's connection
/// (`openrouter::request_extras`): the free models to fall back on, or the
/// data policy with credit. Nothing for another provider or a key pasted
/// in by hand.
pub fn openrouter_request_fields(settings: &AppSettings, provider_id: &str) -> Map<String, Value> {
    match &settings.openrouter_connection {
        Some(connection) if provider_id == OPENROUTER_PROVIDER_ID => {
            openrouter::request_extras(&connection.free_models, connection.free_tier)
        }
        _ => Map::new(),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// The app as the sign-in's host: the browser through the opener, the
/// settings and the sign-in's file, the event to the frontend.
struct TauriHost {
    app: AppHandle,
}

impl TauriHost {
    /// Changes the settings with `change` and saves them, with the effects
    /// a changed provider has (`profile_effects`).
    fn change_settings(
        &self,
        change: impl FnOnce(&mut AppSettings) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut settings = crate::settings::get_settings(&self.app);
        let before = settings.clone();
        change(&mut settings)?;
        crate::settings::write_settings(&self.app, settings.clone());
        crate::profile_effects::apply(&self.app, &before, &settings);
        Ok(())
    }
}

impl SignInHost for TauriHost {
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.app
            .opener()
            .open_url(url, None::<String>)
            .map_err(|error| error.to_string())
    }

    fn chatgpt_identity(&self) -> Result<(Option<String>, String), String> {
        let vault = vault::for_app(&self.app)?;
        let mut settings = crate::settings::get_settings(&self.app);
        let (client_id, host_id, made) = chatgpt_identity_from(vault, &mut settings)?;
        if made {
            crate::settings::write_settings(&self.app, settings);
        }
        Ok((client_id, host_id))
    }

    fn save_chatgpt_client_id(&self, client_id: &str) -> Result<(), String> {
        vault::for_app(&self.app)?
            .update(|secrets| secrets.chatgpt_client_id = Some(client_id.to_string()))
    }

    fn store_chatgpt(&self, session: ChatGptSession) -> Result<(), String> {
        let vault = vault::for_app(&self.app)?;
        self.change_settings(|settings| store_chatgpt_sign_in(vault, settings, &session))
    }

    fn store_openrouter(&self, signed_in: OpenRouterSignedIn) -> Result<(), String> {
        self.change_settings(|settings| apply_openrouter_sign_in(settings, &signed_in, unix_now()))
    }

    fn finished(&self, event: PostProcessSignInFinished) {
        if let Err(error) = event.emit(&self.app) {
            log::error!("Cannot tell the frontend how the sign-in ended: {error}");
        }
    }
}

/// The app's ChatGPT session for the requests through the plan
/// (`session::install`): the sign-in's file, and on a dead refresh token
/// the settings too.
pub struct AppSessionStore {
    app: AppHandle,
}

impl AppSessionStore {
    pub fn new(app: AppHandle) -> Self {
        AppSessionStore { app }
    }
}

impl SessionStore for AppSessionStore {
    fn load(&self) -> Option<ChatGptSession> {
        vault::for_app(&self.app).ok()?.load().chatgpt()
    }

    fn save(&self, session: &ChatGptSession) -> Result<(), String> {
        vault::for_app(&self.app)?.update(|secrets| secrets.chatgpt_session = Some(session.into()))
    }

    fn expire(&self) {
        let ended = vault::for_app(&self.app).and_then(|vault| {
            let mut settings = crate::settings::get_settings(&self.app);
            end_chatgpt_sign_in(vault, &mut settings)?;
            crate::settings::write_settings(&self.app, settings);
            Ok(())
        });
        if let Err(error) = ended {
            log::error!("Cannot end the expired ChatGPT sign-in: {error}");
        }
    }
}

fn host(app: &AppHandle) -> Arc<dyn SignInHost> {
    Arc::new(TauriHost { app: app.clone() })
}

/// Changes the app's settings with `change` and saves them, with the
/// effects a changed provider or model has (for `openrouter_tier`).
pub fn change_app_settings(
    app: &AppHandle,
    change: impl FnOnce(&mut AppSettings) -> Result<(), String>,
) -> Result<(), String> {
    TauriHost { app: app.clone() }.change_settings(change)
}

/// Starts signing in through the browser: binds the loopback listener and
/// opens the provider's page in the browser. `page` holds the two short
/// texts the browser shows after the callback, in the app's language.
/// `post-process-sign-in-finished` says how it ended.
#[tauri::command]
#[specta::specta]
pub fn start_post_process_sign_in(
    app: AppHandle,
    sign_ins: State<'_, SignIns>,
    provider: SignInProvider,
    page: CallbackPage,
) -> Result<SignInStarted, String> {
    let auth_url = sign_ins.start(
        provider,
        page,
        host(&app),
        Endpoints::real(),
        SIGN_IN_TIMEOUT,
    )?;
    Ok(SignInStarted { auth_url })
}

/// Opens the page of the sign-in in progress in the browser again.
#[tauri::command]
#[specta::specta]
pub fn reopen_post_process_sign_in(
    app: AppHandle,
    sign_ins: State<'_, SignIns>,
) -> Result<(), String> {
    sign_ins.reopen(host(&app).as_ref()).map(|_| ())
}

/// Stops waiting for the sign-in in progress.
#[tauri::command]
#[specta::specta]
pub fn cancel_post_process_sign_in(sign_ins: State<'_, SignIns>) {
    sign_ins.cancel();
}

/// Forgets the sign-in: ChatGPT's session (its client id and the host id
/// stay for the next sign-in), or OpenRouter's key and connection.
// DanyWhere (2026-11-27): ChatGPT's renewable session is ended at OpenAI
// first, as far as it answers within a few seconds
// (`revoke_chatgpt_session`). A plain comment, so the generated bindings
// keep their text.
#[tauri::command]
#[specta::specta]
pub async fn disconnect_post_process_sign_in(
    app: AppHandle,
    sign_ins: State<'_, SignIns>,
    provider: SignInProvider,
) -> Result<(), String> {
    // A sign-in with the same provider still waiting would bring it back.
    if sign_ins.in_progress() == Some(provider) {
        sign_ins.cancel();
    }
    let host = TauriHost { app: app.clone() };
    match provider {
        SignInProvider::Chatgpt => {
            let vault = vault::for_app(&app)?;
            revoke_chatgpt_session(vault, chatgpt::DISCOVERY, REVOKE_TIMEOUT).await;
            host.change_settings(|settings| end_chatgpt_sign_in(vault, settings))
        }
        SignInProvider::Openrouter => host.change_settings(end_openrouter_sign_in),
    }
}

/// Reads OpenRouter's key info again (`/api/v1/key`), e.g. after topping
/// up, and lets the model follow the tier.
#[tauri::command]
#[specta::specta]
pub async fn refresh_openrouter_connection(
    app: AppHandle,
) -> Result<OpenRouterConnection, FailureDto> {
    let settings = crate::settings::get_settings(&app);
    let layer = crate::post_process_setup::with_defaults(&settings);
    let provider = layer
        .post_process_provider(crate::post_process_setup::OPENROUTER_PROVIDER_ID)
        .cloned();
    let failure = |reason: FailureReason| {
        FailureDto::from_failure(&PostProcessFailure {
            reason,
            provider: provider
                .as_ref()
                .map(|provider| (provider.id.clone(), provider.label.clone())),
            model: None,
            seconds: None,
        })
    };
    let key = layer
        .post_process_api_keys
        .get(crate::post_process_setup::OPENROUTER_PROVIDER_ID)
        .map(|key| key.trim().to_string())
        .unwrap_or_default();
    if key.is_empty() {
        return Err(failure(FailureReason::NoApiKey));
    }
    let client = token_client().map_err(|error| failure(FailureReason::other(&error)))?;
    let state = openrouter_state(&client, openrouter::DEFAULT_BASE, &key)
        .await
        .map_err(|error| {
            failure(crate::post_process_failure::classify_llm_error(
                &error, true,
            ))
        })?;
    let mut connection = None;
    TauriHost { app: app.clone() }
        .change_settings(|settings| {
            connection = Some(apply_openrouter_refresh(settings, &state, unix_now())?);
            Ok(())
        })
        .map_err(|error| failure(FailureReason::other(&error)))?;
    connection.ok_or_else(|| failure(FailureReason::Unknown))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

    const STORED_MODELS: &str = include_str!("testdata/openrouter_models.json");
    const NEW_KEY: &str = "sk-or-v1-new-key-for-the-test";
    const ISSUED: &str = "oaiapp_issued_for_the_test";
    const HOST_ID: &str = "urn:uuid:3f2a6c1e-8d4b-4c7a-9e21-5b6d7f8a9c0d";
    const PLAN_SCOPES: &str =
        "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
    /// Long enough for any test's callback, short enough to notice a hang.
    const WAIT: Duration = Duration::from_secs(60);

    fn page() -> CallbackPage {
        CallbackPage {
            success: "Connected.".to_string(),
            failure: "That did not work.".to_string(),
        }
    }

    // --- a fake host ---------------------------------------------------------

    struct FakeHost {
        open_fails: bool,
        opened: Mutex<Vec<String>>,
        client_id: Mutex<Option<String>>,
        host_id: Mutex<Option<String>>,
        calls: Mutex<Vec<&'static str>>,
        saved_client_ids: Mutex<Vec<String>>,
        chatgpt: Mutex<Option<ChatGptSession>>,
        openrouter: Mutex<Option<OpenRouterSignedIn>>,
        events: UnboundedSender<PostProcessSignInFinished>,
    }

    fn fake_host(
        client_id: Option<&str>,
        open_fails: bool,
    ) -> (Arc<FakeHost>, UnboundedReceiver<PostProcessSignInFinished>) {
        let (events, received) = unbounded_channel();
        let host = FakeHost {
            open_fails,
            opened: Mutex::new(Vec::new()),
            client_id: Mutex::new(client_id.map(str::to_string)),
            host_id: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
            saved_client_ids: Mutex::new(Vec::new()),
            chatgpt: Mutex::new(None),
            openrouter: Mutex::new(None),
            events,
        };
        (Arc::new(host), received)
    }

    impl SignInHost for FakeHost {
        fn open_url(&self, url: &str) -> Result<(), String> {
            if self.open_fails {
                return Err("no browser".to_string());
            }
            self.opened.lock().unwrap().push(url.to_string());
            Ok(())
        }

        fn chatgpt_identity(&self) -> Result<(Option<String>, String), String> {
            self.calls.lock().unwrap().push("identity");
            let mut host_id = self.host_id.lock().unwrap();
            let id = host_id.get_or_insert_with(|| HOST_ID.to_string()).clone();
            Ok((self.client_id.lock().unwrap().clone(), id))
        }

        fn save_chatgpt_client_id(&self, client_id: &str) -> Result<(), String> {
            self.calls.lock().unwrap().push("save_client_id");
            self.saved_client_ids
                .lock()
                .unwrap()
                .push(client_id.to_string());
            *self.client_id.lock().unwrap() = Some(client_id.to_string());
            Ok(())
        }

        fn store_chatgpt(&self, session: ChatGptSession) -> Result<(), String> {
            self.calls.lock().unwrap().push("store_chatgpt");
            *self.chatgpt.lock().unwrap() = Some(session);
            Ok(())
        }

        fn store_openrouter(&self, signed_in: OpenRouterSignedIn) -> Result<(), String> {
            self.calls.lock().unwrap().push("store_openrouter");
            *self.openrouter.lock().unwrap() = Some(signed_in);
            Ok(())
        }

        fn finished(&self, event: PostProcessSignInFinished) {
            let _ = self.events.send(event);
        }
    }

    async fn next_event(
        events: &mut UnboundedReceiver<PostProcessSignInFinished>,
    ) -> PostProcessSignInFinished {
        tokio::time::timeout(WAIT, events.recv())
            .await
            .expect("the sign-in ends in time")
            .expect("an event")
    }

    /// No further event within a moment.
    async fn no_more_events(events: &mut UnboundedReceiver<PostProcessSignInFinished>) {
        let more = tokio::time::timeout(Duration::from_millis(400), events.recv()).await;
        assert!(more.is_err(), "unexpected event: {more:?}");
    }

    // --- a fake server -------------------------------------------------------

    #[derive(Debug, Clone)]
    struct Seen {
        /// "POST /api/v1/auth/keys"
        line: String,
        body: String,
    }

    type Handler = Arc<dyn Fn(&Seen) -> (String, String) + Send + Sync>;

    /// Answers every request with what `handler` says (status, which may
    /// carry header lines after it, and body), and keeps the requests.
    async fn serve(handler: Handler) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut request = Vec::new();
                let mut buffer = [0_u8; 8192];
                let mut got = None;
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
                            let first = text.lines().next().unwrap_or("");
                            let line = first.rsplit_once(' ').map_or(first, |(l, _)| l);
                            got = Some(Seen {
                                line: line.to_string(),
                                body: text[end + 4..].to_string(),
                            });
                            break;
                        }
                    }
                }
                let Some(seen) = got else { continue };
                kept.lock().unwrap().push(seen.clone());
                let (status, body) = handler(&seen);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        (format!("http://{address}"), seen)
    }

    fn lines(seen: &Arc<Mutex<Vec<Seen>>>) -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .map(|s| s.line.clone())
            .collect()
    }

    fn key_info_body(free_tier: bool) -> String {
        json!({"data": {
            "label": "sk-or-v1-abc...xyz", "limit": 10, "limit_remaining": 10, "usage": 0,
            "is_free_tier": free_tier,
            "free_model_daily_requests": {"used": 0, "limit": 50, "remaining": 50}
        }})
        .to_string()
    }

    /// OpenRouter: the exchange, the key info and the model list.
    fn openrouter_handler(free_tier: bool, models_status: &'static str) -> Handler {
        Arc::new(move |seen: &Seen| match seen.line.as_str() {
            "POST /api/v1/auth/keys" => (
                "200 OK".to_string(),
                json!({"key": NEW_KEY, "user_id": "user_test"}).to_string(),
            ),
            "GET /api/v1/key" => ("200 OK".to_string(), key_info_body(free_tier)),
            "GET /api/v1/models" => (models_status.to_string(), STORED_MODELS.to_string()),
            _ => ("404 Not Found".to_string(), "{}".to_string()),
        })
    }

    fn endpoints(openrouter_base: &str, chatgpt_base: &str) -> Endpoints {
        Endpoints {
            openrouter_base: openrouter_base.to_string(),
            chatgpt_token_url: format!("{chatgpt_base}/api/accounts/oauth/token"),
        }
    }

    fn query_of(url: &str) -> HashMap<String, String> {
        reqwest::Url::parse(url)
            .expect("url parses")
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    /// Plays the browser: calls back on the redirect URI named by
    /// `callback_param` in `auth_url`, with the state and `params`.
    async fn play_browser(
        auth_url: &str,
        callback_param: &str,
        params: &[(&str, &str)],
    ) -> Result<reqwest::StatusCode, reqwest::Error> {
        let query = query_of(auth_url);
        let redirect = query.get(callback_param).expect("a redirect URI").clone();
        let mut pairs: Vec<(&str, &str)> = params.to_vec();
        pairs.push(("state", query.get("state").expect("a state")));
        let url = reqwest::Url::parse_with_params(&redirect, &pairs).unwrap();
        reqwest::Client::new()
            .get(url)
            .send()
            .await
            .map(|response| response.status())
    }

    fn jwt(claims: &Value) -> String {
        let header = json!({"alg": "RS256", "typ": "JWT"});
        format!(
            "{}.{}.c2lnbmF0dXJl",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    /// ChatGPT's token endpoint: a good answer for the nonce the test puts
    /// into `nonce` once it knows the page's URL, with `scope`.
    fn chatgpt_handler(nonce: Arc<Mutex<String>>, scope: &'static str) -> Handler {
        Arc::new(move |seen: &Seen| {
            if seen.line != "POST /api/accounts/oauth/token" {
                return ("404 Not Found".to_string(), "{}".to_string());
            }
            let form = query_of(&format!("http://x/?{}", seen.body));
            let client = form.get("client_id").cloned().unwrap_or_default();
            let id_token = jwt(&json!({
                "iss": chatgpt::ISSUER, "aud": [client], "sub": "user-sub-1",
                "email": "jo@example.com", "exp": now() + 3600, "iat": now(),
                "nonce": nonce.lock().unwrap().clone(),
            }));
            let answer = json!({
                "access_token": "access-1", "refresh_token": "refresh-1", "id_token": id_token,
                "token_type": "Bearer", "expires_in": 3600, "scope": scope,
            });
            ("200 OK".to_string(), answer.to_string())
        })
    }

    fn failing_handler(status: &'static str) -> Handler {
        Arc::new(move |_| {
            (
                status.to_string(),
                json!({"error": "server_error"}).to_string(),
            )
        })
    }

    // --- the guards ------------------------------------------------------------

    #[test]
    fn the_copied_error_prefixes_are_the_ones_of_llm_client() {
        assert_eq!(
            openrouter::STATUS_ERROR_PREFIX,
            crate::llm_client::STATUS_ERROR_PREFIX
        );
        assert_eq!(
            crate::llm_responses::STATUS_ERROR_PREFIX,
            crate::llm_client::STATUS_ERROR_PREFIX
        );
        assert_eq!(
            crate::llm_responses::TRUNCATED_ERROR_PREFIX,
            crate::llm_client::TRUNCATED_ERROR_PREFIX
        );
    }

    #[test]
    fn the_real_endpoints_are_openrouter_and_openai() {
        let real = Endpoints::real();
        assert_eq!(real.openrouter_base, openrouter::DEFAULT_BASE);
        assert_eq!(real.chatgpt_token_url, chatgpt::TOKEN);
        assert_eq!(SIGN_IN_TIMEOUT, Duration::from_secs(1800));
    }

    #[test]
    fn the_model_is_the_first_free_one_or_the_paid_one() {
        let free = vec!["a:free".to_string(), "b:free".to_string()];
        assert_eq!(openrouter_model(true, &free), "a:free");
        assert_eq!(openrouter_model(true, &[]), "");
        assert_eq!(openrouter_model(false, &free), openrouter::PAID_MODEL);
        assert_eq!(openrouter_model(false, &[]), openrouter::PAID_MODEL);
    }

    #[test]
    fn the_debug_text_of_an_openrouter_sign_in_hides_the_key() {
        let signed_in = OpenRouterSignedIn {
            key: NEW_KEY.to_string(),
            state: OpenRouterState {
                info: KeyInfo {
                    free_tier: true,
                    limit_usd: Some(10.0),
                    usage_usd: 0.0,
                    label: None,
                    limit_remaining_usd: None,
                    free_requests_remaining: None,
                },
                free_models: vec!["a:free".to_string()],
                model: "a:free".to_string(),
            },
        };
        let text = format!("{signed_in:?}");
        assert!(!text.contains(NEW_KEY), "{text}");
        assert!(text.contains("a:free"), "{text}");
    }

    #[test]
    fn the_finished_event_reads_as_the_frontend_expects() {
        let event = PostProcessSignInFinished {
            provider: SignInProvider::Openrouter,
            ok: true,
            error: None,
            free_tier: Some(true),
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"provider": "openrouter", "ok": true, "error": null, "free_tier": true})
        );
        assert_eq!(
            serde_json::to_value(SignInProvider::Chatgpt).unwrap(),
            json!("chatgpt")
        );
    }

    #[tokio::test]
    async fn the_token_client_does_not_follow_redirects() {
        let (elsewhere, elsewhere_seen) =
            serve(Arc::new(|_| ("200 OK".to_string(), "{}".to_string()))).await;
        let moved = format!("307 Temporary Redirect\r\nLocation: {elsewhere}/stolen");
        let (base, seen) = serve(Arc::new(move |_| (moved.clone(), "{}".to_string()))).await;
        let client = token_client().unwrap();
        let response = client
            .post(format!("{base}/api/accounts/oauth/token"))
            .body("refresh_token=secret")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 307);
        assert_eq!(lines(&seen).len(), 1);
        assert!(
            lines(&elsewhere_seen).is_empty(),
            "the token went elsewhere"
        );
    }

    // --- OpenRouter --------------------------------------------------------------

    #[tokio::test]
    async fn an_openrouter_sign_in_exchanges_the_code_and_stores_key_and_free_models() {
        let (base, seen) = serve(openrouter_handler(true, "200 OK")).await;
        let (host, mut events) = fake_host(None, false);
        let sign_ins = SignIns::default();
        let auth_url = sign_ins
            .start(
                SignInProvider::Openrouter,
                page(),
                host.clone(),
                endpoints(&base, &base),
                WAIT,
            )
            .expect("the sign-in starts");
        assert!(
            auth_url.starts_with(openrouter::AUTHORIZE_URL),
            "{auth_url}"
        );
        assert_eq!(*host.opened.lock().unwrap(), vec![auth_url.clone()]);
        assert_eq!(sign_ins.in_progress(), Some(SignInProvider::Openrouter));

        let status = play_browser(&auth_url, "callback_url", &[("code", "code-123")])
            .await
            .unwrap();
        assert_eq!(status.as_u16(), 200);

        let event = next_event(&mut events).await;
        assert_eq!(
            event,
            PostProcessSignInFinished {
                provider: SignInProvider::Openrouter,
                ok: true,
                error: None,
                free_tier: Some(true),
            }
        );
        let stored = host.openrouter.lock().unwrap().take().expect("stored");
        assert_eq!(stored.key, NEW_KEY);
        let expected = openrouter::choose_free_models(
            &serde_json::from_str(STORED_MODELS).unwrap(),
            true,
            openrouter::MAX_FALLBACK_MODELS,
        );
        assert!(!expected.is_empty());
        assert_eq!(stored.state.free_models, expected);
        assert_eq!(stored.state.model, expected[0]);
        assert!(stored.state.info.free_tier);
        assert_eq!(stored.state.info.limit_usd, Some(10.0));

        // The code went with the verifier whose challenge the page carried.
        let exchange = seen
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.line == "POST /api/v1/auth/keys")
            .cloned()
            .expect("the code was exchanged");
        let body: Value = serde_json::from_str(&exchange.body).unwrap();
        assert_eq!(body["code"], "code-123");
        let verifier = body["code_verifier"].as_str().unwrap().to_string();
        assert_eq!(
            loopback::Pkce::from_verifier(verifier).challenge,
            query_of(&auth_url)["code_challenge"]
        );
        assert_eq!(sign_ins.in_progress(), None);
        assert!(sign_ins.reopen(host.as_ref()).is_err());
    }

    #[tokio::test]
    async fn an_openrouter_account_with_credit_gets_the_paid_model() {
        let (base, _seen) = serve(openrouter_handler(false, "200 OK")).await;
        let (host, mut events) = fake_host(None, false);
        let sign_ins = SignIns::default();
        let auth_url = sign_ins
            .start(
                SignInProvider::Openrouter,
                page(),
                host.clone(),
                endpoints(&base, &base),
                WAIT,
            )
            .unwrap();
        play_browser(&auth_url, "callback_url", &[("code", "c")])
            .await
            .unwrap();
        let event = next_event(&mut events).await;
        assert!(event.ok, "{event:?}");
        assert_eq!(event.free_tier, Some(false));
        let stored = host.openrouter.lock().unwrap().take().expect("stored");
        assert_eq!(stored.state.model, openrouter::PAID_MODEL);
        assert!(!stored.state.info.free_tier);
    }

    #[tokio::test]
    async fn without_openrouters_model_list_the_preferred_free_models_are_tried() {
        let (base, _seen) = serve(openrouter_handler(true, "503 Service Unavailable")).await;
        let client = token_client().unwrap();
        let state = openrouter_state(&client, &base, NEW_KEY)
            .await
            .expect("the key info is enough");
        let preferred: Vec<String> = openrouter::FREE_MODEL_PREFERENCE
            .iter()
            .take(openrouter::MAX_FALLBACK_MODELS)
            .map(|model| model.to_string())
            .collect();
        assert_eq!(state.free_models, preferred);
        assert_eq!(state.model, preferred[0]);
    }

    #[tokio::test]
    async fn a_refused_openrouter_exchange_ends_the_sign_in_with_its_error() {
        let (base, _seen) = serve(failing_handler("403 Forbidden")).await;
        let (host, mut events) = fake_host(None, false);
        let sign_ins = SignIns::default();
        let auth_url = sign_ins
            .start(
                SignInProvider::Openrouter,
                page(),
                host.clone(),
                endpoints(&base, &base),
                WAIT,
            )
            .unwrap();
        play_browser(&auth_url, "callback_url", &[("code", "c")])
            .await
            .unwrap();
        let event = next_event(&mut events).await;
        assert!(!event.ok);
        let error = event.error.expect("an error");
        assert!(
            error.starts_with(crate::llm_client::STATUS_ERROR_PREFIX),
            "{error}"
        );
        assert!(host.openrouter.lock().unwrap().is_none());
    }

    // --- ChatGPT -----------------------------------------------------------------

    #[tokio::test]
    async fn a_first_chatgpt_sign_in_registers_saves_the_client_and_stores_the_session() {
        let nonce = Arc::new(Mutex::new(String::new()));
        let (base, seen) = serve(chatgpt_handler(nonce.clone(), PLAN_SCOPES)).await;
        let (host, mut events) = fake_host(None, false);
        let sign_ins = SignIns::default();
        let auth_url = sign_ins
            .start(
                SignInProvider::Chatgpt,
                page(),
                host.clone(),
                endpoints(&base, &base),
                WAIT,
            )
            .unwrap();
        let query = query_of(&auth_url);
        assert!(auth_url.starts_with(chatgpt::AUTHORIZE), "{auth_url}");
        assert_eq!(query["client_id"], chatgpt::DYNAMIC_CLIENT);
        assert_eq!(query["ext_agent_host_id"], HOST_ID);
        assert!(query["redirect_uri"].starts_with("http://127.0.0.1:"));
        *nonce.lock().unwrap() = query["nonce"].clone();

        play_browser(
            &auth_url,
            "redirect_uri",
            &[("code", "code-9"), ("client_id", ISSUED)],
        )
        .await
        .unwrap();
        let event = next_event(&mut events).await;
        assert_eq!(
            event,
            PostProcessSignInFinished {
                provider: SignInProvider::Chatgpt,
                ok: true,
                error: None,
                free_tier: None,
            }
        );
        assert_eq!(
            *host.calls.lock().unwrap(),
            vec!["identity", "save_client_id", "store_chatgpt"]
        );
        assert_eq!(*host.saved_client_ids.lock().unwrap(), vec![ISSUED]);
        let session = host.chatgpt.lock().unwrap().clone().expect("stored");
        assert_eq!(session.client_id, ISSUED);
        assert_eq!(session.access_token, "access-1");
        assert_eq!(session.refresh_token, "refresh-1");
        let token = seen.lock().unwrap()[0].clone();
        let form = query_of(&format!("http://x/?{}", token.body));
        assert_eq!(form["client_id"], ISSUED);
        assert_eq!(form["code"], "code-9");
        assert_eq!(form["redirect_uri"], query["redirect_uri"]);
    }

    #[tokio::test]
    async fn the_issued_client_id_is_saved_even_when_the_exchange_fails() {
        let (base, _seen) = serve(failing_handler("500 Internal Server Error")).await;
        let (host, mut events) = fake_host(None, false);
        let sign_ins = SignIns::default();
        let auth_url = sign_ins
            .start(
                SignInProvider::Chatgpt,
                page(),
                host.clone(),
                endpoints(&base, &base),
                WAIT,
            )
            .unwrap();
        play_browser(
            &auth_url,
            "redirect_uri",
            &[("code", "c"), ("client_id", ISSUED)],
        )
        .await
        .unwrap();
        let event = next_event(&mut events).await;
        assert!(!event.ok);
        assert!(
            event
                .error
                .as_deref()
                .is_some_and(|e| e.starts_with(crate::llm_client::STATUS_ERROR_PREFIX)),
            "{event:?}"
        );
        assert_eq!(*host.saved_client_ids.lock().unwrap(), vec![ISSUED]);
        assert!(host.chatgpt.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn a_later_chatgpt_sign_in_uses_the_saved_client() {
        let nonce = Arc::new(Mutex::new(String::new()));
        let (base, _seen) = serve(chatgpt_handler(nonce.clone(), PLAN_SCOPES)).await;
        let (host, mut events) = fake_host(Some(ISSUED), false);
        let sign_ins = SignIns::default();
        let auth_url = sign_ins
            .start(
                SignInProvider::Chatgpt,
                page(),
                host.clone(),
                endpoints(&base, &base),
                WAIT,
            )
            .unwrap();
        let query = query_of(&auth_url);
        assert_eq!(query["client_id"], ISSUED);
        *nonce.lock().unwrap() = query["nonce"].clone();
        play_browser(&auth_url, "redirect_uri", &[("code", "c")])
            .await
            .unwrap();
        let event = next_event(&mut events).await;
        assert!(event.ok, "{event:?}");
        assert!(host.saved_client_ids.lock().unwrap().is_empty());
        assert_eq!(
            host.chatgpt
                .lock()
                .unwrap()
                .as_ref()
                .map(|s| s.client_id.clone()),
            Some(ISSUED.to_string())
        );
    }

    #[tokio::test]
    async fn a_chatgpt_sign_in_without_plan_usage_is_not_eligible() {
        let nonce = Arc::new(Mutex::new(String::new()));
        let (base, _seen) = serve(chatgpt_handler(
            nonce.clone(),
            "openid profile email offline_access",
        ))
        .await;
        let (host, mut events) = fake_host(Some(ISSUED), false);
        let sign_ins = SignIns::default();
        let auth_url = sign_ins
            .start(
                SignInProvider::Chatgpt,
                page(),
                host.clone(),
                endpoints(&base, &base),
                WAIT,
            )
            .unwrap();
        *nonce.lock().unwrap() = query_of(&auth_url)["nonce"].clone();
        play_browser(&auth_url, "redirect_uri", &[("code", "c")])
            .await
            .unwrap();
        let event = next_event(&mut events).await;
        assert!(!event.ok);
        assert_eq!(event.error.as_deref(), Some(ERROR_NOT_ELIGIBLE));
        assert!(host.chatgpt.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn a_refusal_in_the_browser_ends_the_sign_in_as_denied() {
        let (base, seen) = serve(failing_handler("500 Internal Server Error")).await;
        let (host, mut events) = fake_host(Some(ISSUED), false);
        let sign_ins = SignIns::default();
        let auth_url = sign_ins
            .start(
                SignInProvider::Chatgpt,
                page(),
                host.clone(),
                endpoints(&base, &base),
                WAIT,
            )
            .unwrap();
        play_browser(&auth_url, "redirect_uri", &[("error", "access_denied")])
            .await
            .unwrap();
        let event = next_event(&mut events).await;
        assert!(!event.ok);
        assert_eq!(event.error.as_deref(), Some(ERROR_DENIED));
        assert!(lines(&seen).is_empty(), "nothing was exchanged");
    }

    // --- one sign-in at a time ---------------------------------------------------

    #[tokio::test]
    async fn cancelling_ends_the_sign_in_as_cancelled() {
        let (host, mut events) = fake_host(None, false);
        let sign_ins = SignIns::default();
        sign_ins
            .start(
                SignInProvider::Openrouter,
                page(),
                host.clone(),
                endpoints("http://127.0.0.1:9", "http://127.0.0.1:9"),
                WAIT,
            )
            .unwrap();
        sign_ins.cancel();
        let event = next_event(&mut events).await;
        assert_eq!(
            event,
            PostProcessSignInFinished {
                provider: SignInProvider::Openrouter,
                ok: false,
                error: Some(ERROR_CANCELLED.to_string()),
                free_tier: None,
            }
        );
        assert_eq!(sign_ins.in_progress(), None);
        assert!(sign_ins.reopen(host.as_ref()).is_err());
    }

    #[tokio::test]
    async fn a_sign_in_without_callback_times_out() {
        let (host, mut events) = fake_host(None, false);
        let sign_ins = SignIns::default();
        sign_ins
            .start(
                SignInProvider::Chatgpt,
                page(),
                host.clone(),
                endpoints("http://127.0.0.1:9", "http://127.0.0.1:9"),
                Duration::from_millis(300),
            )
            .unwrap();
        let event = next_event(&mut events).await;
        assert_eq!(event.provider, SignInProvider::Chatgpt);
        assert!(!event.ok);
        assert_eq!(event.error.as_deref(), Some(ERROR_TIMED_OUT));
        assert_eq!(sign_ins.in_progress(), None);
    }

    #[tokio::test]
    async fn a_second_start_replaces_the_first_without_an_event_for_it() {
        let (base, _seen) = serve(openrouter_handler(true, "200 OK")).await;
        let (host, mut events) = fake_host(None, false);
        let sign_ins = SignIns::default();
        let first = sign_ins
            .start(
                SignInProvider::Openrouter,
                page(),
                host.clone(),
                endpoints(&base, &base),
                WAIT,
            )
            .unwrap();
        let second = sign_ins
            .start(
                SignInProvider::Openrouter,
                page(),
                host.clone(),
                endpoints(&base, &base),
                WAIT,
            )
            .unwrap();
        assert_ne!(first, second);
        // The first one's listener closes.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            play_browser(&first, "callback_url", &[("code", "c")])
                .await
                .is_err(),
            "the first sign-in still listens"
        );
        play_browser(&second, "callback_url", &[("code", "c")])
            .await
            .unwrap();
        let event = next_event(&mut events).await;
        assert!(event.ok, "{event:?}");
        no_more_events(&mut events).await;
        assert_eq!(sign_ins.in_progress(), None);
    }

    #[tokio::test]
    async fn reopening_opens_the_same_page_again() {
        let (host, mut events) = fake_host(None, false);
        let sign_ins = SignIns::default();
        let auth_url = sign_ins
            .start(
                SignInProvider::Openrouter,
                page(),
                host.clone(),
                endpoints("http://127.0.0.1:9", "http://127.0.0.1:9"),
                WAIT,
            )
            .unwrap();
        assert_eq!(sign_ins.reopen(host.as_ref()), Ok(auth_url.clone()));
        assert_eq!(
            *host.opened.lock().unwrap(),
            vec![auth_url.clone(), auth_url]
        );
        assert_eq!(sign_ins.in_progress(), Some(SignInProvider::Openrouter));
        sign_ins.cancel();
        let _ = next_event(&mut events).await;
    }

    #[tokio::test]
    async fn without_a_sign_in_reopening_fails_and_cancelling_does_nothing() {
        let (host, mut events) = fake_host(None, false);
        let sign_ins = SignIns::default();
        assert!(sign_ins.reopen(host.as_ref()).is_err());
        sign_ins.cancel();
        no_more_events(&mut events).await;
        assert!(host.opened.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_browser_that_cannot_open_leaves_no_sign_in() {
        let (host, mut events) = fake_host(None, true);
        let sign_ins = SignIns::default();
        let started = sign_ins.start(
            SignInProvider::Openrouter,
            page(),
            host.clone(),
            endpoints("http://127.0.0.1:9", "http://127.0.0.1:9"),
            WAIT,
        );
        let error = started.expect_err("no browser, no sign-in");
        assert!(error.contains("no browser"), "{error}");
        assert_eq!(sign_ins.in_progress(), None);
        no_more_events(&mut events).await;
    }

    // --- the app's side: settings and the sign-in's file ------------------------

    use crate::post_process_setup::{post_process_ready, with_defaults, Readiness};
    use crate::profile_layers::{self, Scope};
    use crate::settings::{get_default_settings, PostProcessMode};

    /// New-install settings whose defaults never post-process, with a
    /// profile "Work" that follows them.
    fn new_install() -> AppSettings {
        let mut settings = get_default_settings();
        crate::profiles::ensure_profiles(&mut settings);
        let profile = crate::profiles::profile_for_test(&settings, "work", "Work", |_| {});
        settings.profiles.push(profile);
        profile_layers::set_value(
            &mut settings,
            &Scope::Defaults,
            "post_process_mode",
            serde_json::to_value(PostProcessMode::Never).unwrap(),
        )
        .unwrap();
        settings
    }

    fn chatgpt_session() -> ChatGptSession {
        ChatGptSession {
            client_id: ISSUED.to_string(),
            access_token: "chatgpt-access-secret-1".to_string(),
            refresh_token: "chatgpt-refresh-secret-1".to_string(),
            expires_at: now() + 3600,
            email: Some("someone@example.com".to_string()),
            plan: Some("plus".to_string()),
            subject: "user-subject-secret".to_string(),
            earliest_refresh_at: None,
        }
    }

    fn signed_in_openrouter(free_tier: bool) -> OpenRouterSignedIn {
        let free_models = vec![
            "nvidia/nemotron-3.5-super-vl:free".to_string(),
            "inclusionai/ling-3.2-flash:free".to_string(),
        ];
        OpenRouterSignedIn {
            key: NEW_KEY.to_string(),
            state: OpenRouterState {
                info: KeyInfo {
                    free_tier,
                    limit_usd: Some(10.0),
                    usage_usd: 0.0,
                    label: None,
                    limit_remaining_usd: Some(10.0),
                    free_requests_remaining: Some(50),
                },
                model: openrouter_model(free_tier, &free_models),
                free_models,
            },
        }
    }

    fn defaults_entry(settings: &AppSettings, field: &str, id: &str) -> Option<String> {
        let layer = with_defaults(settings);
        let map = match field {
            "models" => &layer.post_process_models,
            "efforts" => &layer.post_process_efforts,
            _ => panic!("{field}"),
        };
        map.get(id).cloned()
    }

    fn defaults_key(settings: &AppSettings, id: &str) -> Option<String> {
        with_defaults(settings)
            .post_process_api_keys
            .get(id)
            .cloned()
    }

    #[test]
    fn a_chatgpt_sign_in_sets_up_the_plan_in_the_defaults_and_makes_it_usable() {
        let mut settings = new_install();
        apply_chatgpt_sign_in(&mut settings).expect("applies");
        let layer = with_defaults(&settings);
        assert!(settings.chatgpt_signed_in);
        assert_eq!(layer.post_process_provider_id, "chatgpt");
        assert_eq!(
            defaults_entry(&settings, "models", "chatgpt").as_deref(),
            Some(chatgpt::DEFAULT_MODEL)
        );
        assert_eq!(
            defaults_entry(&settings, "efforts", "chatgpt").as_deref(),
            Some(chatgpt::DEFAULT_EFFORT)
        );
        assert_eq!(post_process_ready(&layer), Readiness::Ready);
        assert_eq!(layer.post_process_mode, PostProcessMode::WithInstructionKey);
        assert!(!crate::prompt_library::requested(&layer).is_empty());
        // The profile that follows the defaults post-processes with it too.
        assert_eq!(
            settings.profiles[0].values.post_process_provider_id,
            "chatgpt"
        );
    }

    #[test]
    fn after_a_chatgpt_sign_in_the_settings_hold_no_token() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path().join(vault::FILE_NAME));
        let mut settings = new_install();
        store_chatgpt_sign_in(&vault, &mut settings, &chatgpt_session()).expect("stores");
        let sent = serde_json::to_string(&settings).unwrap();
        for secret in [
            "chatgpt-access-secret-1",
            "chatgpt-refresh-secret-1",
            "user-subject-secret",
            "someone@example.com",
            "access_token",
            "refresh_token",
        ] {
            assert!(!sent.contains(secret), "{secret} in the settings");
        }
        let kept = vault.load();
        assert_eq!(kept.chatgpt_client_id.as_deref(), Some(ISSUED));
        let session = kept.chatgpt().expect("the session is in the file");
        assert_eq!(session.refresh_token, "chatgpt-refresh-secret-1");
        assert!(settings.chatgpt_signed_in);
    }

    #[test]
    fn the_host_id_is_made_once_and_the_client_id_comes_from_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path().join(vault::FILE_NAME));
        let mut settings = new_install();
        let (client, host_id, made) = chatgpt_identity_from(&vault, &mut settings).unwrap();
        assert_eq!(client, None);
        assert!(made);
        assert!(host_id.starts_with("urn:uuid:"), "{host_id}");
        assert_eq!(
            settings.chatgpt_host_id.as_ref().map(|id| id.0.clone()),
            Some(host_id.clone())
        );
        vault
            .update(|secrets| secrets.chatgpt_client_id = Some(ISSUED.to_string()))
            .unwrap();
        let (client, again, made) = chatgpt_identity_from(&vault, &mut settings).unwrap();
        assert_eq!(client.as_deref(), Some(ISSUED));
        assert_eq!(again, host_id);
        assert!(!made);
    }

    #[test]
    fn an_ended_chatgpt_sign_in_keeps_client_and_host_id() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path().join(vault::FILE_NAME));
        let mut settings = new_install();
        let (_, host_id, _) = chatgpt_identity_from(&vault, &mut settings).unwrap();
        store_chatgpt_sign_in(&vault, &mut settings, &chatgpt_session()).unwrap();
        end_chatgpt_sign_in(&vault, &mut settings).expect("ends");
        assert!(!settings.chatgpt_signed_in);
        let kept = vault.load();
        assert!(kept.chatgpt().is_none(), "the tokens are gone");
        assert_eq!(kept.chatgpt_client_id.as_deref(), Some(ISSUED));
        assert_eq!(
            settings.chatgpt_host_id.as_ref().map(|id| id.0.clone()),
            Some(host_id)
        );
        assert_eq!(
            post_process_ready(&with_defaults(&settings)),
            Readiness::SignIn
        );
        let text = std::fs::read_to_string(vault.path()).unwrap();
        assert!(!text.contains("chatgpt-refresh-secret-1"), "{text}");
    }

    #[test]
    fn an_openrouter_sign_in_puts_key_model_and_connection_into_the_settings() {
        let mut settings = new_install();
        let signed_in = signed_in_openrouter(true);
        apply_openrouter_sign_in(&mut settings, &signed_in, 1_795_300_000).expect("applies");
        let layer = with_defaults(&settings);
        assert_eq!(layer.post_process_provider_id, "openrouter");
        assert_eq!(
            defaults_key(&settings, "openrouter").as_deref(),
            Some(NEW_KEY)
        );
        assert_eq!(
            defaults_entry(&settings, "models", "openrouter").as_deref(),
            Some("nvidia/nemotron-3.5-super-vl:free")
        );
        assert_eq!(
            settings.openrouter_connection,
            Some(OpenRouterConnection {
                free_tier: true,
                limit_usd: Some(10.0),
                free_models: signed_in.state.free_models.clone(),
                checked_at: 1_795_300_000,
            })
        );
        assert_eq!(post_process_ready(&layer), Readiness::Ready);
        assert_eq!(layer.post_process_mode, PostProcessMode::WithInstructionKey);
    }

    #[test]
    fn after_topping_up_the_model_follows_the_tier_unless_chosen_by_hand() {
        let mut settings = new_install();
        apply_openrouter_sign_in(&mut settings, &signed_in_openrouter(true), 1).unwrap();
        let paid = signed_in_openrouter(false).state;
        let connection = apply_openrouter_refresh(&mut settings, &paid, 2).expect("refreshes");
        assert!(!connection.free_tier);
        assert_eq!(connection.checked_at, 2);
        assert_eq!(settings.openrouter_connection, Some(connection));
        assert_eq!(
            defaults_entry(&settings, "models", "openrouter").as_deref(),
            Some(openrouter::PAID_MODEL)
        );
        // A model chosen by hand stays.
        profile_layers::set_entry(
            &mut settings,
            &Scope::Defaults,
            "post_process_models",
            "openrouter",
            serde_json::json!("mistralai/some-model"),
        )
        .unwrap();
        apply_openrouter_refresh(&mut settings, &signed_in_openrouter(true).state, 3).unwrap();
        assert_eq!(
            defaults_entry(&settings, "models", "openrouter").as_deref(),
            Some("mistralai/some-model")
        );
    }

    #[test]
    fn an_ended_openrouter_sign_in_forgets_key_and_connection() {
        let mut settings = new_install();
        apply_openrouter_sign_in(&mut settings, &signed_in_openrouter(true), 1).unwrap();
        end_openrouter_sign_in(&mut settings).expect("ends");
        assert_eq!(defaults_key(&settings, "openrouter").as_deref(), Some(""));
        assert_eq!(settings.openrouter_connection, None);
        assert_eq!(
            post_process_ready(&with_defaults(&settings)),
            Readiness::Key
        );
    }

    #[test]
    fn an_openrouter_request_gets_the_fields_of_its_connection() {
        let mut settings = new_install();
        assert!(openrouter_request_fields(&settings, "openrouter").is_empty());
        apply_openrouter_sign_in(&mut settings, &signed_in_openrouter(true), 1).unwrap();
        let free = openrouter_request_fields(&settings, "openrouter");
        assert_eq!(
            Value::Object(free),
            serde_json::json!({"models": [
                "nvidia/nemotron-3.5-super-vl:free", "inclusionai/ling-3.2-flash:free"
            ]})
        );
        assert!(openrouter_request_fields(&settings, "groq").is_empty());
        apply_openrouter_refresh(&mut settings, &signed_in_openrouter(false).state, 2).unwrap();
        let paid = openrouter_request_fields(&settings, "openrouter");
        assert_eq!(
            Value::Object(paid),
            serde_json::json!({"provider": {"data_collection": "deny"}})
        );
    }

    // --- signing out of ChatGPT ------------------------------------------------

    /// OpenAI's discovery document naming a revocation endpoint on the fake
    /// server, and the revocation answering `revoke_status`.
    fn revocation_handler(base: Arc<Mutex<String>>, revoke_status: &'static str) -> Handler {
        Arc::new(move |seen: &Seen| {
            match seen.line.as_str() {
            "GET /.well-known/openid-configuration" => (
                "200 OK".to_string(),
                json!({
                    "issuer": base.lock().unwrap().clone(),
                    "token_endpoint": format!("{}/api/accounts/oauth/token", base.lock().unwrap()),
                    "revocation_endpoint": format!("{}/api/accounts/oauth/revoke", base.lock().unwrap()),
                })
                .to_string(),
            ),
            "POST /api/accounts/oauth/revoke" => (revoke_status.to_string(), String::new()),
            _ => ("404 Not Found".to_string(), "{}".to_string()),
        }
        })
    }

    fn signed_in_vault() -> (tempfile::TempDir, Vault, AppSettings) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path().join(vault::FILE_NAME));
        let mut settings = new_install();
        store_chatgpt_sign_in(&vault, &mut settings, &chatgpt_session()).unwrap();
        (dir, vault, settings)
    }

    #[tokio::test]
    async fn signing_out_of_chatgpt_revokes_the_refresh_token_before_forgetting_it() {
        let base = Arc::new(Mutex::new(String::new()));
        let (url, seen) = serve(revocation_handler(base.clone(), "200 OK")).await;
        *base.lock().unwrap() = url.clone();
        let (_dir, vault, mut settings) = signed_in_vault();

        let discovery = format!("{url}/.well-known/openid-configuration");
        assert!(revoke_chatgpt_session(&vault, &discovery, Duration::from_secs(5)).await);
        assert_eq!(
            lines(&seen),
            vec![
                "GET /.well-known/openid-configuration",
                "POST /api/accounts/oauth/revoke"
            ]
        );
        let form = query_of(&format!("http://x/?{}", seen.lock().unwrap()[1].body));
        assert_eq!(
            form.get("token").map(String::as_str),
            Some("chatgpt-refresh-secret-1")
        );
        assert_eq!(
            form.get("token_type_hint").map(String::as_str),
            Some("refresh_token")
        );
        assert_eq!(form.get("client_id").map(String::as_str), Some(ISSUED));
        // The session was still here while it was revoked; then it goes.
        assert!(vault.load().chatgpt().is_some());
        end_chatgpt_sign_in(&vault, &mut settings).unwrap();
        assert!(vault.load().chatgpt().is_none());
    }

    #[tokio::test]
    async fn a_revocation_openai_refuses_or_never_gets_still_signs_out_here() {
        let base = Arc::new(Mutex::new(String::new()));
        let (url, seen) = serve(revocation_handler(base.clone(), "503 Service Unavailable")).await;
        *base.lock().unwrap() = url.clone();
        let (_dir, vault, mut settings) = signed_in_vault();
        let discovery = format!("{url}/.well-known/openid-configuration");
        assert!(!revoke_chatgpt_session(&vault, &discovery, Duration::from_secs(5)).await);
        assert_eq!(lines(&seen).len(), 2);
        // Nobody listens there.
        let gone = "http://127.0.0.1:9/.well-known/openid-configuration";
        assert!(!revoke_chatgpt_session(&vault, gone, Duration::from_secs(2)).await);
        end_chatgpt_sign_in(&vault, &mut settings).unwrap();
        assert!(vault.load().chatgpt().is_none());
        assert!(!settings.chatgpt_signed_in);
    }

    #[tokio::test]
    async fn without_a_chatgpt_session_nothing_is_revoked() {
        let base = Arc::new(Mutex::new(String::new()));
        let (url, seen) = serve(revocation_handler(base.clone(), "200 OK")).await;
        *base.lock().unwrap() = url.clone();
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path().join(vault::FILE_NAME));
        let discovery = format!("{url}/.well-known/openid-configuration");
        assert!(!revoke_chatgpt_session(&vault, &discovery, Duration::from_secs(5)).await);
        assert!(lines(&seen).is_empty(), "{:?}", lines(&seen));
    }
}
