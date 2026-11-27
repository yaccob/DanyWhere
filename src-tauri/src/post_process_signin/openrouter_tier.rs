//! DanyWhere: OpenRouter's tier, asked again by itself (sidecar note
//! `2026-11-26_2250_nachbearbeitung-einstieg-umsetzung`, section 2, "OpenRouter
//! mit Guthaben"). Once the account has bought credit, `GET /api/v1/key` no
//! longer reports the free tier, and post-processing switches to
//! [`openrouter::PAID_MODEL`] with `provider.data_collection = "deny"`, as
//! "I have topped up" does (`commands::refresh_openrouter_connection`): not
//! when the user picked a model by hand, and only for a connection without
//! an intent (stored before the setup asked). Signed in for the free models
//! it stays with them; signed in for credit it has the paid model from the
//! start (review of 2026-11-27, H1 and M1; `commands::openrouter_model`).
//!
//! When it asks: after a post-processing request through a connected
//! OpenRouter key on the free tier (at most once every
//! [`REQUEST_CHECK_GAP`]), otherwise at most once every [`CHECK_INTERVAL`],
//! and once [`FIRST_CHECK_DELAY`] after the app starts. A thread of its own
//! asks ([`TierWatch`]); a request only wakes it ([`AfterRequest`]), so no
//! dictation waits for it. Its request has a short time limit of its own
//! ([`CHECK_TIMEOUT`]), and a failure is only logged.
//!
//! The free models' daily limit: a 429 of a request on the free tier whose
//! answer doesn't name that limit (OpenRouter's "Rate limit exceeded", a busy
//! free model) asks the key info once, within [`DAILY_LIMIT_TIMEOUT`]; no
//! free request left today makes it `free-daily-limit` ([`refine_failure`];
//! strand L: `free_requests_remaining == 0` is the safe sign).
//!
//! Every request takes OpenRouter's site as a parameter, so tests point it
//! at a fake server.

use super::commands::{self, OpenRouterState};
use super::openrouter::{self, KeyInfo};
use crate::post_process_failure::{self, FailureReason};
use crate::post_process_setup::{with_defaults, OPENROUTER_PROVIDER_ID};
use crate::settings::{AppSettings, PostProcessProvider};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// How long after the app's start the first check comes.
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
/// How often it asks while no request asks for it.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// The shortest time between two checks when requests ask for them.
pub const REQUEST_CHECK_GAP: Duration = Duration::from_secs(60);
/// The time limit of a check's request.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(5);
/// The time limit of the key info after a 429; the dictation's failure waits
/// for it.
pub const DAILY_LIMIT_TIMEOUT: Duration = Duration::from_secs(3);

/// When checks are due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierTimes {
    /// After the start, the first check.
    pub first: Duration,
    /// Between two checks without a request.
    pub interval: Duration,
    /// At least between two checks a request asks for.
    pub gap: Duration,
}

impl TierTimes {
    /// The app's times.
    pub const APP: TierTimes = TierTimes {
        first: FIRST_CHECK_DELAY,
        interval: CHECK_INTERVAL,
        gap: REQUEST_CHECK_GAP,
    };
}

/// When the next check is due: [`TierTimes::first`] after the start, then
/// every [`TierTimes::interval`]; a request makes it due at once, or
/// [`TierTimes::gap`] after the last check.
#[derive(Debug, Clone)]
pub struct Schedule {
    times: TierTimes,
    started: Instant,
    last_check: Option<Instant>,
    requested: bool,
}

impl Schedule {
    pub fn new(times: TierTimes, started: Instant) -> Self {
        Schedule {
            times,
            started,
            last_check: None,
            requested: false,
        }
    }

    /// A request went out: a check is wanted.
    pub fn request(&mut self) {
        self.requested = true;
    }

    /// When the next check is due.
    pub fn next_due(&self) -> Instant {
        let periodic = match self.last_check {
            Some(last) => last + self.times.interval,
            None => self.started + self.times.first,
        };
        if !self.requested {
            return periodic;
        }
        let asked = match self.last_check {
            Some(last) => last + self.times.gap,
            None => self.started,
        };
        periodic.min(asked)
    }

    /// A check started at `now`.
    pub fn checked(&mut self, now: Instant) {
        self.last_check = Some(now);
        self.requested = false;
    }
}

/// The thread that asks: [`TierWatch::run`] waits until a check is due and
/// runs it; [`TierWatch::request_sent`] only wakes it.
pub struct TierWatch {
    schedule: Mutex<Schedule>,
    wake: Condvar,
    stopped: AtomicBool,
}

impl TierWatch {
    pub fn new(times: TierTimes) -> Arc<Self> {
        Arc::new(TierWatch {
            schedule: Mutex::new(Schedule::new(times, Instant::now())),
            wake: Condvar::new(),
            stopped: AtomicBool::new(false),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Schedule> {
        self.schedule.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A post-processing request went out on the free tier. Never waits for
    /// a check.
    pub fn request_sent(&self) {
        // The worker holds the lock only to read the schedule, never during
        // a check.
        self.lock().request();
        self.wake.notify_all();
    }

    /// Ends [`TierWatch::run`]; the app's watch runs as long as the app.
    #[cfg(test)]
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.wake.notify_all();
    }

    /// Runs `check` whenever one is due, until [`TierWatch::stop`].
    pub fn run(&self, mut check: impl FnMut()) {
        loop {
            {
                let mut schedule = self.lock();
                loop {
                    if self.stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    let now = Instant::now();
                    let due = schedule.next_due();
                    if now >= due {
                        schedule.checked(now);
                        break;
                    }
                    schedule = self
                        .wake
                        .wait_timeout(schedule, due - now)
                        .map(|(guard, _)| guard)
                        .unwrap_or_else(|poisoned| poisoned.into_inner().0);
                }
            }
            check();
        }
    }
}

/// Whether a request with `settings` to `provider_id` asks for a check: an
/// OpenRouter key connected through the sign-in, on the free tier, with
/// its key.
pub fn wants_check(settings: &AppSettings, provider_id: &str) -> bool {
    provider_id == OPENROUTER_PROVIDER_ID
        && settings
            .openrouter_connection
            .as_ref()
            .is_some_and(|connection| connection.free_tier)
        && !openrouter_key(settings).is_empty()
}

/// Whether a request with `settings` to `provider_id` goes to OpenRouter's
/// free models (`commands::openrouter_request_fields`): a connected key on
/// the free tier, or signed in for the free models (also with credit);
/// never when signed in for credit (review of 2026-11-27, H1 and M1).
pub fn uses_free_models(settings: &AppSettings, provider_id: &str) -> bool {
    provider_id == OPENROUTER_PROVIDER_ID
        && settings
            .openrouter_connection
            .as_ref()
            .is_some_and(|connection| match commands::connection_intent(connection) {
                Some(commands::SignInIntent::Credit) => false,
                Some(commands::SignInIntent::Free) => true,
                None => connection.free_tier,
            })
        && !openrouter_key(settings).is_empty()
}

/// The OpenRouter key in `settings`.
fn openrouter_key(settings: &AppSettings) -> String {
    settings
        .post_process_api_keys
        .get(OPENROUTER_PROVIDER_ID)
        .map(|key| key.trim().to_string())
        .unwrap_or_default()
}

/// OpenRouter's site for a provider's `base_url` ("https://openrouter.ai/api/v1"
/// gives "https://openrouter.ai"); [`openrouter::DEFAULT_BASE`] for any
/// other.
pub fn site_of(base_url: &str) -> String {
    match base_url
        .trim()
        .trim_end_matches('/')
        .strip_suffix("/api/v1")
    {
        Some(site) if !site.is_empty() => site.to_string(),
        _ => openrouter::DEFAULT_BASE.to_string(),
    }
}

/// The client of these requests: no redirects (they carry the key) and
/// `timeout` for each.
pub fn tier_client(timeout: Duration) -> Result<reqwest::Client, String> {
    commands::short_token_client(timeout)
}

/// Asks OpenRouter at `site` about the key in `settings` when its
/// connection is on the free tier: `Some` with the key info when the account
/// has credit now, `None` while it is still free; without such a connection
/// it asks nothing.
pub async fn check_tier(
    client: &reqwest::Client,
    site: &str,
    settings: &AppSettings,
) -> Result<Option<KeyInfo>, String> {
    if !wants_check(settings, OPENROUTER_PROVIDER_ID) {
        return Ok(None);
    }
    let info = openrouter::key_info(client, site, &openrouter_key(settings)).await?;
    Ok((!info.free_tier).then_some(info))
}

/// After [`check_tier`] found credit at `now` (Unix seconds): the connection
/// anew, and the defaults' OpenRouter model becomes
/// [`openrouter::PAID_MODEL`] unless the user chose another
/// (`commands::apply_openrouter_refresh`). Returns whether it changed
/// anything.
pub fn apply_paid_tier(
    settings: &mut AppSettings,
    info: &KeyInfo,
    now: i64,
) -> Result<bool, String> {
    let Some(connection) = settings.openrouter_connection.clone() else {
        return Ok(false);
    };
    if !connection.free_tier || info.free_tier {
        return Ok(false);
    }
    // The free models stay in the connection: the model the sign-in chose
    // from them counts as DanyWhere's, not as one chosen by hand. Signed in
    // for the free models, the model stays a free one
    // (`commands::openrouter_model`).
    let intent = commands::connection_intent(&connection);
    let state = OpenRouterState {
        info: info.clone(),
        model: commands::openrouter_model(intent, false, &connection.free_models),
        free_models: connection.free_models,
        intent,
    };
    commands::apply_openrouter_refresh(settings, &state, now)?;
    Ok(true)
}

/// The reason of a failed request with `settings` to `provider_id`, after
/// asking the key info at `site` within `timeout` when it may be the free
/// models' daily limit (`post_process_failure::may_be_free_daily_limit`).
pub async fn refine_failure_at(
    settings: &AppSettings,
    provider_id: &str,
    error: &str,
    reason: FailureReason,
    site: &str,
    timeout: Duration,
) -> FailureReason {
    if !uses_free_models(settings, provider_id)
        || !post_process_failure::may_be_free_daily_limit(error, &reason)
    {
        return reason;
    }
    let client = match tier_client(timeout) {
        Ok(client) => client,
        Err(error) => {
            log::warn!("Cannot ask OpenRouter for the free requests left: {error}");
            return reason;
        }
    };
    match openrouter::key_info(&client, site, &openrouter_key(settings)).await {
        Ok(info) => {
            let refined = post_process_failure::with_free_requests_left(
                reason.clone(),
                info.free_requests_remaining,
            );
            if refined != reason {
                log::info!(
                    "OpenRouter: no free request is left today, so the 429 ({reason:?}) is the free models' daily limit"
                );
            }
            refined
        }
        Err(error) => {
            log::warn!(
                "Cannot read OpenRouter's free requests left: {}",
                post_process_failure::redact_secrets(&error)
            );
            reason
        }
    }
}

/// [`refine_failure_at`] at `provider`'s site within
/// [`DAILY_LIMIT_TIMEOUT`].
pub async fn refine_failure(
    settings: &AppSettings,
    provider: &PostProcessProvider,
    error: &str,
    reason: FailureReason,
) -> FailureReason {
    refine_failure_at(
        settings,
        &provider.id,
        error,
        reason,
        &site_of(&provider.base_url),
        DAILY_LIMIT_TIMEOUT,
    )
    .await
}

static INSTALLED: OnceLock<Arc<TierWatch>> = OnceLock::new();

/// Tells the app's [`TierWatch`], when this guard ends, that a request went
/// out, if [`wants_check`] says it asks for a check. Made right before a
/// post-processing request, it ends after it, however the request ends
/// (also on the time limit).
#[must_use]
pub struct AfterRequest {
    watch: Option<Arc<TierWatch>>,
}

impl AfterRequest {
    /// For the app's watch, started by [`start`]; without it (tests) a
    /// guard that does nothing.
    pub fn new(settings: &AppSettings, provider_id: &str) -> Self {
        Self::with_watch(settings, provider_id, INSTALLED.get().cloned())
    }

    pub fn with_watch(
        settings: &AppSettings,
        provider_id: &str,
        watch: Option<Arc<TierWatch>>,
    ) -> Self {
        AfterRequest {
            watch: watch.filter(|_| wants_check(settings, provider_id)),
        }
    }
}

impl Drop for AfterRequest {
    fn drop(&mut self) {
        if let Some(watch) = self.watch.take() {
            watch.request_sent();
        }
    }
}

/// Starts the app's watch in a thread of its own; its first check comes
/// [`FIRST_CHECK_DELAY`] after now.
pub fn start(app: AppHandle) {
    let watch = TierWatch::new(TierTimes::APP);
    if INSTALLED.set(watch.clone()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("openrouter-tier".to_string())
        .spawn(move || watch.run(|| check_now(&app)));
    if let Err(error) = spawned {
        log::error!("Cannot watch OpenRouter's tier: {error}");
    }
}

/// One check with the app's settings; switches to the paid model when the
/// account has credit now.
fn check_now(app: &AppHandle) {
    let defaults = with_defaults(&crate::settings::get_settings(app));
    if !wants_check(&defaults, OPENROUTER_PROVIDER_ID) {
        return;
    }
    let site = defaults
        .post_process_provider(OPENROUTER_PROVIDER_ID)
        .map(|provider| site_of(&provider.base_url))
        .unwrap_or_else(|| openrouter::DEFAULT_BASE.to_string());
    let client = match tier_client(CHECK_TIMEOUT) {
        Ok(client) => client,
        Err(error) => {
            log::warn!("Cannot ask OpenRouter about the tier: {error}");
            return;
        }
    };
    match tauri::async_runtime::block_on(check_tier(&client, &site, &defaults)) {
        Ok(None) => log::debug!("OpenRouter: still on the free tier"),
        Ok(Some(info)) => {
            let mut switched = false;
            let changed = commands::change_app_settings(app, |settings| {
                switched = apply_paid_tier(settings, &info, unix_now())?;
                Ok(())
            });
            match changed {
                Ok(()) if switched => log::info!(
                    "OpenRouter has credit now: post-processing goes to {} unless a model was chosen by hand",
                    openrouter::PAID_MODEL
                ),
                Ok(()) => {}
                Err(error) => log::warn!("Cannot switch to OpenRouter's paid model: {error}"),
            }
        }
        Err(error) => log::warn!(
            "Asking OpenRouter about the tier failed: {}",
            post_process_failure::redact_secrets(&error)
        ),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::post_process_setup::OpenRouterConnection;
    use crate::profile_layers::{self, Scope};
    use crate::settings::get_default_settings;
    use serde_json::{json, Value};
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const KEY: &str = "sk-or-v1-key-for-the-tier-test";
    const FREE: [&str; 2] = [
        "nvidia/nemotron-3.5-super-vl:free",
        "inclusionai/ling-3.2-flash:free",
    ];
    const NOW: i64 = 1_795_400_000;

    fn free_models() -> Vec<String> {
        FREE.iter().map(|model| model.to_string()).collect()
    }

    /// Settings after an OpenRouter sign-in on the free tier or with credit,
    /// without an intent (as stored before the setup asked).
    fn signed_in(free_tier: bool) -> AppSettings {
        signed_in_for(None, free_tier)
    }

    /// Settings after an OpenRouter sign-in for `intent`.
    fn signed_in_for(intent: Option<commands::SignInIntent>, free_tier: bool) -> AppSettings {
        let mut settings = get_default_settings();
        crate::profiles::ensure_profiles(&mut settings);
        let free_models = free_models();
        let signed_in = commands::OpenRouterSignedIn {
            key: KEY.to_string(),
            state: OpenRouterState {
                info: key_info(free_tier, Some(50)),
                model: commands::openrouter_model(intent, free_tier, &free_models),
                free_models,
                intent,
            },
        };
        commands::apply_openrouter_sign_in(&mut settings, &signed_in, 1_795_300_000)
            .expect("signs in");
        settings
    }

    fn key_info(free_tier: bool, remaining: Option<u32>) -> KeyInfo {
        KeyInfo {
            free_tier,
            limit_usd: Some(10.0),
            usage_usd: 0.0,
            label: None,
            limit_remaining_usd: Some(10.0),
            free_requests_remaining: remaining,
        }
    }

    fn defaults_model(settings: &AppSettings) -> Option<String> {
        with_defaults(settings)
            .post_process_models
            .get(OPENROUTER_PROVIDER_ID)
            .cloned()
    }

    /// `GET /api/v1/key`'s answer, as on 2026-11-26.
    fn key_body(free_tier: bool, remaining: u32) -> String {
        json!({"data": {
            "label": "sk-or-v1-abc...xyz", "limit": 10, "limit_remaining": 10, "usage": 0,
            "is_free_tier": free_tier,
            "free_model_daily_requests": {"used": 50 - remaining.min(50), "limit": 50, "remaining": remaining}
        }})
        .to_string()
    }

    /// One request as the fake server saw it: "GET /api/v1/key" and its
    /// `Authorization`.
    #[derive(Debug, Clone)]
    struct Seen {
        line: String,
        authorization: Option<String>,
    }

    type Handler = Arc<dyn Fn(&str) -> (String, String) + Send + Sync>;

    /// Answers every request by its request line with what `handler` says
    /// (status and body), and keeps the requests.
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
                let mut head = None;
                loop {
                    let n = stream.read(&mut buffer).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..n]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        head = Some(text[..end].to_string());
                        break;
                    }
                }
                let Some(head) = head else { continue };
                let first = head.lines().next().unwrap_or("");
                let line = first.rsplit_once(' ').map_or(first, |(l, _)| l).to_string();
                let authorization = head.lines().find_map(|header| {
                    let (name, value) = header.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("authorization")
                        .then(|| value.trim().to_string())
                });
                kept.lock().unwrap().push(Seen {
                    line: line.clone(),
                    authorization,
                });
                let (status, body) = handler(&line);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        (format!("http://{address}"), seen)
    }

    /// The key info as `key_body` says.
    fn key_handler(free_tier: bool, remaining: u32) -> Handler {
        Arc::new(move |line: &str| match line {
            "GET /api/v1/key" => ("200 OK".to_string(), key_body(free_tier, remaining)),
            _ => ("404 Not Found".to_string(), "{}".to_string()),
        })
    }

    fn lines(seen: &Arc<Mutex<Vec<Seen>>>) -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .map(|s| s.line.clone())
            .collect()
    }

    fn client() -> reqwest::Client {
        tier_client(Duration::from_secs(5)).expect("a client")
    }

    // --- when it asks ----------------------------------------------------------

    const TIMES: TierTimes = TierTimes {
        first: Duration::from_secs(20),
        interval: Duration::from_secs(3600),
        gap: Duration::from_secs(60),
    };

    #[test]
    fn a_new_schedule_is_due_shortly_after_the_start_then_every_hour() {
        let start = Instant::now();
        let mut schedule = Schedule::new(TIMES, start);
        assert_eq!(schedule.next_due(), start + TIMES.first);
        let first = start + TIMES.first;
        schedule.checked(first);
        assert_eq!(schedule.next_due(), first + TIMES.interval);
    }

    #[test]
    fn a_request_makes_a_check_due_at_once_unless_one_just_ran() {
        let start = Instant::now();
        // Before the first check: at once.
        let mut schedule = Schedule::new(TIMES, start);
        schedule.request();
        assert_eq!(schedule.next_due(), start);
        // A minute after the last check at the earliest.
        let checked = start + Duration::from_secs(30);
        schedule.checked(checked);
        assert_eq!(schedule.next_due(), checked + TIMES.interval);
        schedule.request();
        assert_eq!(schedule.next_due(), checked + TIMES.gap);
        // The check it asked for ran: back to the hour.
        let later = checked + TIMES.gap;
        schedule.checked(later);
        assert_eq!(schedule.next_due(), later + TIMES.interval);
    }

    #[test]
    fn a_request_wakes_the_watch_at_once_and_never_waits_for_its_check() {
        let watch = TierWatch::new(TierTimes {
            first: Duration::from_secs(3600),
            interval: Duration::from_secs(3600),
            gap: Duration::ZERO,
        });
        let checks = Arc::new(AtomicUsize::new(0));
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let running = watch.clone();
        let counted = checks.clone();
        let worker = std::thread::spawn(move || {
            running.run(|| {
                counted.fetch_add(1, Ordering::SeqCst);
                // A check that takes long: until the test releases it.
                let _ = hold.recv_timeout(Duration::from_secs(10));
            })
        });
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(checks.load(Ordering::SeqCst), 0, "no check before an hour");

        let asked = Instant::now();
        watch.request_sent();
        assert!(asked.elapsed() < Duration::from_secs(1));
        let waited = Instant::now();
        while checks.load(Ordering::SeqCst) == 0 && waited.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            checks.load(Ordering::SeqCst),
            1,
            "the request woke the watch"
        );

        // While that check runs (it holds for 10 s), a request returns at
        // once; the margin is for a busy test machine.
        let asked = Instant::now();
        watch.request_sent();
        assert!(asked.elapsed() < Duration::from_secs(1));

        watch.stop();
        drop(release);
        worker.join().unwrap();
    }

    #[test]
    fn only_a_connected_free_openrouter_key_asks_after_a_request() {
        let free = with_defaults(&signed_in(true));
        assert!(wants_check(&free, "openrouter"));
        assert!(!wants_check(&free, "groq"));
        // With credit the tier can't change any more.
        assert!(!wants_check(
            &with_defaults(&signed_in(false)),
            "openrouter"
        ));
        // A key pasted in by hand has no connection.
        let mut by_hand = free.clone();
        by_hand.openrouter_connection = None;
        assert!(!wants_check(&by_hand, "openrouter"));
        // No key, no check.
        let mut no_key = free.clone();
        no_key
            .post_process_api_keys
            .insert("openrouter".to_string(), "  ".to_string());
        assert!(!wants_check(&no_key, "openrouter"));
    }

    #[test]
    fn the_guard_tells_the_watch_after_the_request() {
        let free = with_defaults(&signed_in(true));
        let watch = TierWatch::new(TIMES);
        let guard = AfterRequest::with_watch(&free, "openrouter", Some(watch.clone()));
        assert!(!watch.lock().requested, "not before the request ended");
        drop(guard);
        assert!(watch.lock().requested);

        let other = TierWatch::new(TIMES);
        drop(AfterRequest::with_watch(&free, "groq", Some(other.clone())));
        assert!(!other.lock().requested);
    }

    #[test]
    fn the_site_is_the_providers_base_without_the_api_path() {
        assert_eq!(
            site_of("https://openrouter.ai/api/v1"),
            "https://openrouter.ai"
        );
        assert_eq!(
            site_of("https://openrouter.ai/api/v1/"),
            "https://openrouter.ai"
        );
        assert_eq!(
            site_of("http://127.0.0.1:4711/api/v1"),
            "http://127.0.0.1:4711"
        );
        assert_eq!(site_of("https://example.com/v1"), openrouter::DEFAULT_BASE);
    }

    // --- the check -------------------------------------------------------------

    #[tokio::test]
    async fn a_free_account_that_stays_free_changes_nothing() {
        let (site, seen) = serve(key_handler(true, 40)).await;
        let settings = with_defaults(&signed_in(true));
        let found = check_tier(&client(), &site, &settings).await;
        assert_eq!(found, Ok(None));
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].line, "GET /api/v1/key");
        assert_eq!(
            seen[0].authorization.as_deref(),
            Some(format!("Bearer {KEY}").as_str())
        );
    }

    #[tokio::test]
    async fn after_topping_up_the_check_switches_to_the_paid_model_and_denies_data_collection() {
        let (site, _) = serve(key_handler(false, 1000)).await;
        let mut settings = signed_in(true);
        assert_eq!(defaults_model(&settings).as_deref(), Some(FREE[0]));
        let info = check_tier(&client(), &site, &with_defaults(&settings))
            .await
            .expect("asks")
            .expect("credit now");
        assert!(!info.free_tier);
        assert_eq!(apply_paid_tier(&mut settings, &info, NOW), Ok(true));
        assert_eq!(
            defaults_model(&settings).as_deref(),
            Some(openrouter::PAID_MODEL)
        );
        let connection = settings.openrouter_connection.clone().expect("connected");
        assert!(!connection.free_tier);
        assert_eq!(connection.checked_at, NOW);
        assert_eq!(
            Value::Object(commands::openrouter_request_fields(&settings, "openrouter")),
            json!({"provider": {"data_collection": "deny"}})
        );
        // Once paid, nothing more to switch.
        assert_eq!(apply_paid_tier(&mut settings, &info, NOW + 1), Ok(false));
    }

    #[test]
    fn a_model_chosen_by_hand_stays_when_the_account_tops_up() {
        let mut settings = signed_in(true);
        profile_layers::set_entry(
            &mut settings,
            &Scope::Defaults,
            "post_process_models",
            "openrouter",
            json!("mistralai/some-model"),
        )
        .unwrap();
        assert_eq!(
            apply_paid_tier(&mut settings, &key_info(false, None), NOW),
            Ok(true)
        );
        assert_eq!(
            defaults_model(&settings).as_deref(),
            Some("mistralai/some-model")
        );
        assert_eq!(
            settings.openrouter_connection.map(|c| c.free_tier),
            Some(false)
        );
    }

    #[test]
    fn a_free_answer_switches_nothing() {
        let mut settings = signed_in(true);
        let before = settings.clone();
        assert_eq!(
            apply_paid_tier(&mut settings, &key_info(true, Some(3)), NOW),
            Ok(false)
        );
        assert_eq!(
            serde_json::to_value(&settings).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
    }

    #[tokio::test]
    async fn without_a_free_connection_nothing_is_asked() {
        let (site, seen) = serve(key_handler(true, 40)).await;
        let paid = with_defaults(&signed_in(false));
        let mut by_hand = with_defaults(&signed_in(true));
        by_hand.openrouter_connection = None;
        for settings in [paid, by_hand] {
            assert_eq!(check_tier(&client(), &site, &settings).await, Ok(None));
        }
        assert!(lines(&seen).is_empty(), "{:?}", lines(&seen));
    }

    #[tokio::test]
    async fn the_check_gives_up_at_its_own_short_timeout() {
        // Accepts and never answers.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let site = format!("http://{}", listener.local_addr().unwrap());
        let held = tokio::spawn(async move {
            let mut open = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                open.push(stream);
            }
        });
        let settings = with_defaults(&signed_in(true));
        let started = Instant::now();
        let found = check_tier(
            &tier_client(Duration::from_millis(300)).unwrap(),
            &site,
            &settings,
        )
        .await;
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        let error = found.expect_err("gives up");
        assert!(error.contains("timeout"), "{error}");
        assert!(!error.contains(KEY), "{error}");
        held.abort();
    }

    // --- the daily limit -------------------------------------------------------

    /// OpenRouter's own rate limit ("Errors and Debugging"), which doesn't
    /// say which limit.
    const RATE_429: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"code":429,"message":"Rate limit exceeded","metadata":{"error_type":"rate_limit_exceeded"}}}"#;
    /// A busy free model (2026-11-26).
    const BUSY_429: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"message":"Provider returned error","code":429,"metadata":{"raw":"google/gemma-4-31b-it:free is temporarily rate-limited upstream.","provider_name":"Google AI Studio","limit_source":"upstream_provider_shared_pool"}}}"#;
    const NO_CREDIT_429: &str = r#"API request failed with status 429 Too Many Requests: {"error":{"message":"insufficient_quota","code":429}}"#;
    const SERVER_500: &str = "API request failed with status 500 Internal Server Error: boom";

    async fn refined(
        settings: &AppSettings,
        provider_id: &str,
        error: &str,
        site: &str,
    ) -> FailureReason {
        let reason = post_process_failure::classify_llm_error(error, true);
        refine_failure_at(
            settings,
            provider_id,
            error,
            reason,
            site,
            Duration::from_secs(3),
        )
        .await
    }

    #[tokio::test]
    async fn the_daily_limit_is_named_when_no_free_request_is_left() {
        let (site, seen) = serve(key_handler(true, 0)).await;
        let settings = with_defaults(&signed_in(true));
        for error in [RATE_429, BUSY_429] {
            assert_eq!(
                refined(&settings, "openrouter", error, &site).await,
                FailureReason::FreeDailyLimit,
                "{error}"
            );
        }
        assert_eq!(lines(&seen), vec!["GET /api/v1/key", "GET /api/v1/key"]);
    }

    #[tokio::test]
    async fn with_free_requests_left_a_429_keeps_its_reason() {
        let (site, _) = serve(key_handler(true, 7)).await;
        let settings = with_defaults(&signed_in(true));
        assert_eq!(
            refined(&settings, "openrouter", RATE_429, &site).await,
            FailureReason::RateLimited
        );
        assert_eq!(
            refined(&settings, "openrouter", BUSY_429, &site).await,
            FailureReason::FreeBusy
        );
    }

    #[tokio::test]
    async fn only_a_free_openrouter_429_asks_for_the_key_info() {
        let (site, seen) = serve(key_handler(true, 0)).await;
        let free = with_defaults(&signed_in(true));
        // Another provider, a key with credit, another status, no credit.
        assert_eq!(
            refined(&free, "groq", RATE_429, &site).await,
            FailureReason::RateLimited
        );
        assert_eq!(
            refined(
                &with_defaults(&signed_in(false)),
                "openrouter",
                RATE_429,
                &site
            )
            .await,
            FailureReason::RateLimited
        );
        assert_eq!(
            refined(&free, "openrouter", SERVER_500, &site).await,
            FailureReason::Status(500)
        );
        assert_eq!(
            refined(&free, "openrouter", NO_CREDIT_429, &site).await,
            FailureReason::NoCredit
        );
        assert!(lines(&seen).is_empty(), "{:?}", lines(&seen));
    }

    #[tokio::test]
    async fn a_failed_key_info_keeps_the_reason() {
        let (site, seen) = serve(Arc::new(|_: &str| {
            ("500 Internal Server Error".to_string(), "{}".to_string())
        }))
        .await;
        let settings = with_defaults(&signed_in(true));
        assert_eq!(
            refined(&settings, "openrouter", BUSY_429, &site).await,
            FailureReason::FreeBusy
        );
        assert_eq!(lines(&seen), vec!["GET /api/v1/key"]);
    }

    #[test]
    fn the_connection_in_the_test_settings_is_the_sign_ins() {
        // Guards the helpers above: a free sign-in leaves a free connection
        // with its free models.
        assert_eq!(
            signed_in(true).openrouter_connection,
            Some(OpenRouterConnection {
                free_tier: true,
                limit_usd: Some(10.0),
                free_models: free_models(),
                checked_at: 1_795_300_000,
                paid_only: false,
                free_only: false,
            })
        );
    }

    // --- what the sign-in was for (review of 2026-11-27, H1 and M1) -------

    use commands::SignInIntent;

    #[tokio::test]
    async fn the_tier_check_never_switches_the_free_way_to_the_paid_model() {
        let (site, _) = serve(key_handler(false, 1000)).await;
        let mut settings = signed_in_for(Some(SignInIntent::Free), true);
        assert_eq!(defaults_model(&settings).as_deref(), Some(FREE[0]));
        let info = check_tier(&client(), &site, &with_defaults(&settings))
            .await
            .expect("asks")
            .expect("credit now");
        apply_paid_tier(&mut settings, &info, NOW).expect("applies");
        assert_eq!(defaults_model(&settings).as_deref(), Some(FREE[0]));
        let connection = settings.openrouter_connection.clone().expect("connected");
        assert!(!connection.free_tier, "the account has credit now");
        assert!(connection.free_only, "still the free way");
        // Its requests keep the free models and no data policy.
        let fields = Value::Object(commands::openrouter_request_fields(&settings, "openrouter"));
        assert_eq!(fields, json!({"models": FREE}));
    }

    #[test]
    fn a_connection_without_an_intent_still_follows_the_tier() {
        // Stored by the branch before the intent: no `paid_only`, no
        // `free_only`.
        let stored = json!({
            "free_tier": true, "limit_usd": 10.0,
            "free_models": FREE, "checked_at": 1_795_300_000
        });
        let connection: OpenRouterConnection = serde_json::from_value(stored).expect("loads");
        assert!(!connection.paid_only);
        assert!(!connection.free_only);
        let mut settings = signed_in(true);
        settings.openrouter_connection = Some(connection);
        assert_eq!(
            apply_paid_tier(&mut settings, &key_info(false, None), NOW),
            Ok(true)
        );
        assert_eq!(
            defaults_model(&settings).as_deref(),
            Some(openrouter::PAID_MODEL)
        );
    }

    #[tokio::test]
    async fn on_the_credit_way_a_429_is_never_the_free_models_daily_limit() {
        let (site, seen) = serve(key_handler(true, 0)).await;
        let settings = with_defaults(&signed_in_for(Some(SignInIntent::Credit), true));
        assert_eq!(
            refined(&settings, "openrouter", RATE_429, &site).await,
            FailureReason::RateLimited
        );
        assert!(lines(&seen).is_empty(), "{:?}", lines(&seen));
    }

    #[tokio::test]
    async fn the_free_way_on_an_account_with_credit_still_names_the_daily_limit() {
        let (site, _) = serve(key_handler(false, 0)).await;
        let settings = with_defaults(&signed_in_for(Some(SignInIntent::Free), false));
        assert_eq!(
            refined(&settings, "openrouter", RATE_429, &site).await,
            FailureReason::FreeDailyLimit
        );
    }
}
