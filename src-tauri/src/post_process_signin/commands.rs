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
use super::{SignInCode, SignInFlow};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

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
}
