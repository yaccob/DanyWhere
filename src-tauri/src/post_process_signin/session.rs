//! DanyWhere: ChatGPT's session for each request through the plan
//! (`llm_client` with the Responses format). An access token lasts an hour;
//! less than five minutes before its end it is refreshed first. Every
//! refresh replaces the refresh token, so refreshes run one at a time and
//! the new session is saved before it is used; a dead refresh token ends the
//! sign-in (the tokens go, the client id and the host id stay).
//!
//! Where the session lives is a [`SessionStore`]: the sign-in's own file in
//! the app (installed at start with [`install`]), a fake in tests.

use super::chatgpt::{self, ChatGptError, ChatGptSession};
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

/// The error when no session is stored: read as `sign-in-expired`, so the
/// notice offers to sign in again.
pub const NO_SESSION: &str = "sign-in-expired: no ChatGPT sign-in is stored";

/// Where the session is read and saved.
pub trait SessionStore: Send + Sync {
    /// The stored session, if there is one.
    fn load(&self) -> Option<ChatGptSession>;
    /// Saves a refreshed session.
    fn save(&self, session: &ChatGptSession) -> Result<(), String>;
    /// The refresh token is dead: forgets the tokens, keeps the client id
    /// and the host id.
    fn expire(&self);
    /// The client itself is gone (`invalid_client`): forgets the client id
    /// too, so the next sign-in registers anew (review of 2026-11-27, M3).
    fn forget_client(&self) {}
}

/// The one lock around reading, refreshing and saving the session; signing
/// out takes it too (`commands::sign_out_chatgpt`), so no refresh saves a
/// session after it.
pub(super) fn refresh_lock() -> &'static futures_util::lock::Mutex<()> {
    static LOCK: OnceLock<futures_util::lock::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| futures_util::lock::Mutex::new(()))
}

/// The session of `store`, refreshed first through `token_url` when it is
/// due at `now` (Unix seconds).
pub async fn fresh_session(
    store: &dyn SessionStore,
    client: &reqwest::Client,
    token_url: &str,
    now: i64,
) -> Result<ChatGptSession, String> {
    // Read, refresh and save under one lock: a second caller waits and then
    // finds the session already renewed.
    let _held = refresh_lock().lock().await;
    let Some(session) = store.load() else {
        // Nothing stored (a lost file, say): the sign-in counts as ended, so
        // the setup asks for a new one.
        store.expire();
        return Err(NO_SESSION.to_string());
    };
    if !chatgpt::needs_refresh(&session, now) {
        return Ok(session);
    }
    log::info!("Refreshing the ChatGPT sign-in");
    refresh_held(store, client, token_url, &session).await
}

/// Refreshes `session` through `token_url` and saves it, for a caller that
/// holds [`refresh_lock`]; a dead refresh token ends the sign-in.
async fn refresh_held(
    store: &dyn SessionStore,
    client: &reqwest::Client,
    token_url: &str,
    session: &ChatGptSession,
) -> Result<ChatGptSession, String> {
    match chatgpt::refresh(client, token_url, session, |rotated| store.save(rotated)).await {
        Ok(renewed) => {
            // The new refresh token is kept already (`chatgpt::refresh`);
            // the rest of the session before use. A session that can't be
            // saved fails the request (review of 2026-11-27, N1).
            store.save(&renewed).map_err(|error| {
                log::error!("Cannot save the refreshed ChatGPT sign-in: {error}");
                ChatGptError::NotSaved(error).to_string()
            })?;
            Ok(renewed)
        }
        Err(ChatGptError::SignInExpired(code)) => {
            log::warn!("The ChatGPT sign-in has expired ({code}); it has to be renewed");
            store.expire();
            let expired = ChatGptError::SignInExpired(code);
            if chatgpt::names_dead_client(&expired) {
                // The next sign-in registers anew (review of 2026-11-27, M3).
                store.forget_client();
            }
            Err(expired.to_string())
        }
        Err(error) => {
            log::warn!("Refreshing the ChatGPT sign-in failed: {error}");
            Err(error.to_string())
        }
    }
}

/// After the plan refused `refused`'s access token (review of 2026-11-27,
/// M2): the session to ask with once more. Another request that renewed
/// it meanwhile gives its session; otherwise it is refreshed now, whatever
/// its expiry says. A dead refresh token ends the sign-in.
pub async fn renewed_after_refusal(
    store: &dyn SessionStore,
    client: &reqwest::Client,
    token_url: &str,
    refused: &ChatGptSession,
) -> Result<ChatGptSession, String> {
    let _held = refresh_lock().lock().await;
    let Some(stored) = store.load() else {
        store.expire();
        return Err(NO_SESSION.to_string());
    };
    if stored.access_token != refused.access_token {
        return Ok(stored);
    }
    log::info!("ChatGPT's plan refused the access token; renewing the sign-in once");
    refresh_held(store, client, token_url, &stored).await
}

/// The plan refused `refused` also after renewing it: the sign-in ends,
/// unless another request renewed it meanwhile.
pub async fn end_refused(store: &dyn SessionStore, refused: &ChatGptSession) {
    let _held = refresh_lock().lock().await;
    if store
        .load()
        .is_some_and(|stored| stored.access_token == refused.access_token)
    {
        log::warn!("ChatGPT's plan refused the renewed sign-in; it has to be renewed");
        store.expire();
    }
}

struct Installed {
    store: Arc<dyn SessionStore>,
    token_url: String,
}

static INSTALLED: RwLock<Option<Installed>> = RwLock::new(None);

/// Makes `store` the app's session store, refreshed through `token_url`.
pub fn install(store: Arc<dyn SessionStore>, token_url: String) {
    *INSTALLED.write().unwrap_or_else(PoisonError::into_inner) =
        Some(Installed { store, token_url });
}

/// The app's session store and the token URL it is refreshed through, once
/// installed.
pub fn installed() -> Option<(Arc<dyn SessionStore>, String)> {
    INSTALLED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|installed| (installed.store.clone(), installed.token_url.clone()))
}

/// The app's session, fresh ([`fresh_session`] with the installed store).
pub async fn current() -> Result<ChatGptSession, String> {
    let Some((store, token_url)) = installed() else {
        return Err(NO_SESSION.to_string());
    };
    let client = super::commands::token_client()?;
    fresh_session(store.as_ref(), &client, &token_url, unix_now()).await
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const NOW: i64 = 1_795_300_000;

    fn session(expires_at: i64) -> ChatGptSession {
        ChatGptSession {
            client_id: "app_client_1".to_string(),
            access_token: "access-old".to_string(),
            refresh_token: "refresh-old".to_string(),
            expires_at,
            email: Some("someone@example.com".to_string()),
            plan: None,
            subject: "user-sub-1".to_string(),
            earliest_refresh_at: None,
        }
    }

    #[derive(Default)]
    struct FakeStore {
        session: Mutex<Option<ChatGptSession>>,
        saved: AtomicUsize,
        expired: AtomicBool,
        forgot_client: AtomicBool,
        /// Saving fails (a full disk, say).
        save_fails: AtomicBool,
    }

    impl FakeStore {
        fn with(session: ChatGptSession) -> FakeStore {
            FakeStore {
                session: Mutex::new(Some(session)),
                ..Default::default()
            }
        }
    }

    impl SessionStore for FakeStore {
        fn load(&self) -> Option<ChatGptSession> {
            self.session.lock().unwrap().clone()
        }
        fn save(&self, session: &ChatGptSession) -> Result<(), String> {
            if self.save_fails.load(Ordering::SeqCst) {
                return Err("disk full".to_string());
            }
            self.saved.fetch_add(1, Ordering::SeqCst);
            *self.session.lock().unwrap() = Some(session.clone());
            Ok(())
        }
        fn expire(&self) {
            self.expired.store(true, Ordering::SeqCst);
            *self.session.lock().unwrap() = None;
        }
        fn forget_client(&self) {
            self.forgot_client.store(true, Ordering::SeqCst);
        }
    }

    /// Review of 2026-11-27, M3: after `invalid_client` the client id goes
    /// with the tokens, so the next sign-in registers anew.
    #[tokio::test]
    async fn a_dead_client_is_forgotten_with_the_tokens() {
        let (url, _requests) = token_endpoint(
            "401 Unauthorized",
            json!({"error": "invalid_client"}).to_string(),
        )
        .await;
        let store = FakeStore::with(session(NOW - 10));
        let error = fresh_session(&store, &client(), &url, NOW)
            .await
            .expect_err("expired");
        assert!(error.starts_with("sign-in-expired: "), "{error}");
        assert!(store.expired.load(Ordering::SeqCst));
        assert!(store.forgot_client.load(Ordering::SeqCst));
    }

    /// Review of 2026-11-27, N1.
    #[tokio::test]
    async fn a_refresh_answer_that_is_not_accepted_keeps_its_new_token() {
        let (url, _requests) = token_endpoint(
            "200 OK",
            json!({
                "access_token": "access-new", "refresh_token": "refresh-new",
                "token_type": "Bearer", "expires_in": 3600,
                "scope": "openid profile email offline_access",
            })
            .to_string(),
        )
        .await;
        let store = FakeStore::with(session(NOW + 60));
        let error = fresh_session(&store, &client(), &url, NOW)
            .await
            .expect_err("not eligible");
        assert!(error.starts_with("plan-not-eligible:"), "{error}");
        assert_eq!(
            store.load().map(|kept| kept.refresh_token),
            Some("refresh-new".to_string()),
            "the spent token is not kept"
        );
        assert!(!store.expired.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_renewed_session_that_cannot_be_saved_fails_the_request() {
        let (url, _requests) = token_endpoint("200 OK", renewed()).await;
        let store = FakeStore::with(session(NOW + 60));
        store.save_fails.store(true, Ordering::SeqCst);
        let error = fresh_session(&store, &client(), &url, NOW)
            .await
            .expect_err("not saved");
        assert!(error.contains("disk full"), "{error}");
    }

    #[tokio::test]
    async fn a_dead_refresh_token_keeps_the_client() {
        let (url, _requests) = token_endpoint(
            "400 Bad Request",
            json!({"error": "invalid_grant"}).to_string(),
        )
        .await;
        let store = FakeStore::with(session(NOW - 10));
        fresh_session(&store, &client(), &url, NOW)
            .await
            .expect_err("expired");
        assert!(store.expired.load(Ordering::SeqCst));
        assert!(!store.forgot_client.load(Ordering::SeqCst));
    }

    /// A token endpoint that answers every request with `status` and
    /// `body`; returns its URL and the count of requests.
    async fn token_endpoint(status: &'static str, body: String) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let counted = count.clone();
        tokio::spawn(async move {
            loop {
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
                        let length = text[..end]
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                counted.fetch_add(1, Ordering::SeqCst);
                // A moment, so that a second caller would overtake.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        (format!("http://{address}/api/accounts/oauth/token"), count)
    }

    fn renewed() -> String {
        json!({
            "access_token": "access-new", "refresh_token": "refresh-new",
            "token_type": "Bearer", "expires_in": 3600,
            "scope": "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
        })
        .to_string()
    }

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    #[tokio::test]
    async fn a_session_that_is_not_due_goes_as_it_is() {
        let (url, requests) = token_endpoint("200 OK", renewed()).await;
        let store = FakeStore::with(session(NOW + 3600));
        let fresh = fresh_session(&store, &client(), &url, NOW).await.unwrap();
        assert_eq!(fresh.access_token, "access-old");
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        assert_eq!(store.saved.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_due_session_is_refreshed_and_saved_before_it_is_used() {
        let (url, requests) = token_endpoint("200 OK", renewed()).await;
        let store = FakeStore::with(session(NOW + 60));
        let fresh = fresh_session(&store, &client(), &url, NOW).await.unwrap();
        assert_eq!(fresh.access_token, "access-new");
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        // The new refresh token at once, then the whole session (N1).
        assert_eq!(store.saved.load(Ordering::SeqCst), 2);
        let saved = store.load().unwrap();
        assert_eq!(saved.refresh_token, "refresh-new");
        assert_eq!(saved.access_token, "access-new");
    }

    #[tokio::test]
    async fn two_requests_at_once_refresh_only_once() {
        let (url, requests) = token_endpoint("200 OK", renewed()).await;
        let store = FakeStore::with(session(NOW + 60));
        let client = client();
        let (first, second) = tokio::join!(
            fresh_session(&store, &client, &url, NOW),
            fresh_session(&store, &client, &url, NOW),
        );
        assert_eq!(first.unwrap().access_token, "access-new");
        assert_eq!(second.unwrap().access_token, "access-new");
        assert_eq!(requests.load(Ordering::SeqCst), 1, "one refresh");
    }

    #[tokio::test]
    async fn a_dead_refresh_token_ends_the_sign_in() {
        let (url, _requests) = token_endpoint(
            "400 Bad Request",
            json!({"error": "invalid_grant"}).to_string(),
        )
        .await;
        let store = FakeStore::with(session(NOW - 10));
        let error = fresh_session(&store, &client(), &url, NOW)
            .await
            .expect_err("expired");
        assert!(error.starts_with("sign-in-expired: "), "{error}");
        assert!(store.expired.load(Ordering::SeqCst));
        assert!(store.load().is_none());
    }

    #[tokio::test]
    async fn a_refresh_that_fails_for_now_keeps_the_sign_in() {
        let (url, _requests) = token_endpoint(
            "503 Service Unavailable",
            json!({"error": "unavailable"}).to_string(),
        )
        .await;
        let store = FakeStore::with(session(NOW + 60));
        let error = fresh_session(&store, &client(), &url, NOW)
            .await
            .expect_err("not now");
        assert!(
            error.starts_with(crate::llm_client::STATUS_ERROR_PREFIX),
            "{error}"
        );
        assert!(!store.expired.load(Ordering::SeqCst));
        assert_eq!(store.load().unwrap().refresh_token, "refresh-old");
    }

    #[tokio::test]
    async fn without_a_session_it_asks_to_sign_in() {
        let (url, requests) = token_endpoint("200 OK", renewed()).await;
        let store = FakeStore::default();
        let error = fresh_session(&store, &client(), &url, NOW)
            .await
            .expect_err("no session");
        assert_eq!(error, NO_SESSION);
        assert!(
            store.expired.load(Ordering::SeqCst),
            "the sign-in counts as ended"
        );
        assert_eq!(
            crate::post_process_failure::classify_llm_error(&error, true),
            crate::post_process_failure::FailureReason::SignInExpired
        );
        assert_eq!(requests.load(Ordering::SeqCst), 0);
    }
}
