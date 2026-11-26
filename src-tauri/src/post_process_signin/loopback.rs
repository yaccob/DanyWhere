//! The loopback side of a browser sign-in (RFC 8252, section 7.3): a
//! listener on this computer waits for the provider's callback with the
//! authorization code. PKCE (RFC 7636, S256) and a random `state` tie the
//! callback to the request.
//!
//! Plain `std::net`, since the app's tokio has only `process`: `wait` blocks
//! the calling thread, so call it from a thread of its own.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

/// How often `wait` looks for connections, the cancel flag and the time.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// How long one connection may take to send its request or read the answer.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
/// The most of a request the listener reads.
const MAX_REQUEST_BYTES: usize = 16 * 1024;
/// Tries at finding a port that is free on both loopback addresses.
const BIND_ATTEMPTS: usize = 10;

/// Which loopback name the redirect URI uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackHost {
    /// `http://localhost:<port>`; the listener binds 127.0.0.1 and, where
    /// the computer has it, ::1 on the same port (OpenRouter).
    Localhost,
    /// `http://127.0.0.1:<port>`, IPv4 only (ChatGPT).
    Ip127,
}

/// Where the loopback listener binds and which path the provider calls
/// back on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallbackSpec {
    pub host: CallbackHost,
    pub path: &'static str,
}

/// A PKCE pair with the S256 method: `challenge` is the unpadded base64url
/// SHA-256 of `verifier`. Its `Debug` hides the verifier.
#[derive(Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl std::fmt::Debug for Pkce {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Pkce")
            .field("verifier", &"<hidden>")
            .field("challenge", &self.challenge)
            .finish()
    }
}

impl Pkce {
    /// A fresh pair from 32 random bytes (a 43 character verifier).
    pub fn new() -> Result<Pkce, String> {
        Ok(Pkce::from_verifier(random_base64url::<32>()?))
    }

    /// The pair for a given verifier (RFC 7636, 4.2: `BASE64URL(SHA256(verifier))`).
    pub fn from_verifier(verifier: String) -> Pkce {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Pkce {
            verifier,
            challenge,
        }
    }
}

/// A random `state` for one sign-in: 16 random bytes, unpadded base64url.
pub fn random_state() -> Result<String, String> {
    random_base64url::<16>()
}

/// `N` random bytes from the operating system, unpadded base64url.
fn random_base64url<const N: usize>() -> Result<String, String> {
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).map_err(|error| format!("no random bytes: {error}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Short texts for the browser page after the callback, in the frontend's
/// language.
#[derive(Debug, Clone)]
pub struct CallbackPage {
    pub success: String,
    pub failure: String,
}

/// Why a sign-in ended without a code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInError {
    /// The caller set the cancel flag.
    Cancelled,
    /// No valid callback came in time.
    TimedOut,
    /// The provider answered with `error=` (and `error_description`).
    Denied(String),
    /// The time ran out after a callback with a code but the wrong `state`.
    StateMismatch,
    /// The listener failed.
    Io(String),
}

impl std::fmt::Display for SignInError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignInError::Cancelled => formatter.write_str("sign-in cancelled"),
            SignInError::TimedOut => formatter.write_str("sign-in timed out"),
            SignInError::Denied(reason) => write!(formatter, "sign-in denied: {reason}"),
            SignInError::StateMismatch => {
                formatter.write_str("sign-in callback had the wrong state")
            }
            SignInError::Io(error) => write!(formatter, "sign-in listener failed: {error}"),
        }
    }
}

impl std::error::Error for SignInError {}

/// A bound listener waiting for exactly one callback with the right state.
pub struct PendingSignIn {
    listeners: Vec<TcpListener>,
    path: &'static str,
    state: String,
    pkce: Pkce,
    redirect_uri: String,
    auth_url: String,
    page: CallbackPage,
}

impl std::fmt::Debug for PendingSignIn {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingSignIn")
            .field("redirect_uri", &self.redirect_uri)
            .field("pkce", &self.pkce)
            .finish()
    }
}

impl PendingSignIn {
    /// Binds the listener for `flow`, makes the state and the PKCE pair and
    /// asks `flow` for the URL the browser opens.
    pub fn start(
        flow: &dyn super::SignInFlow,
        page: CallbackPage,
    ) -> Result<PendingSignIn, String> {
        let spec = flow.callback();
        let listeners = bind(spec.host)
            .map_err(|error| format!("cannot listen for the sign-in callback: {error}"))?;
        let port = listeners[0]
            .local_addr()
            .map_err(|error| format!("cannot read the callback port: {error}"))?
            .port();
        for listener in &listeners {
            listener
                .set_nonblocking(true)
                .map_err(|error| format!("cannot set up the callback listener: {error}"))?;
        }
        let host = match spec.host {
            CallbackHost::Localhost => "localhost",
            CallbackHost::Ip127 => "127.0.0.1",
        };
        let redirect_uri = format!("http://{host}:{port}{}", spec.path);
        let state = random_state()?;
        let pkce = Pkce::new()?;
        let auth_url = flow.authorize_url(&redirect_uri, &state, &pkce);
        Ok(PendingSignIn {
            listeners,
            path: spec.path,
            state,
            pkce,
            redirect_uri,
            auth_url,
            page,
        })
    }

    /// The URL the browser opens (also for "open the page again"). Copy it
    /// before `wait`, which takes the sign-in.
    pub fn auth_url(&self) -> &str {
        &self.auth_url
    }

    /// The redirect URI the provider calls back on.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Blocks until the callback, a cancel or the timeout. A request with the
    /// wrong `state`, without `code` or on another path does not end the
    /// wait (it gets 400 or 404); `error=` with the right state, or with
    /// none, ends it as `Denied`. The listener closes when this returns.
    pub fn wait(
        self,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<super::SignInCode, SignInError> {
        let PendingSignIn {
            listeners,
            path,
            state,
            pkce,
            redirect_uri,
            page,
            ..
        } = self;
        let deadline = Instant::now() + timeout;
        let expected = Arc::new(Expected { path, state, page });
        let (sender, verdicts) = mpsc::channel::<Verdict>();
        let mut saw_wrong_state = false;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(SignInError::Cancelled);
            }
            for listener in &listeners {
                loop {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let expected = expected.clone();
                            let sender = sender.clone();
                            // One thread per connection, so that a browser's
                            // idle speculative connection holds up nothing.
                            thread::spawn(move || answer(stream, &expected, &sender));
                        }
                        Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                        Err(error)
                            if matches!(
                                error.kind(),
                                ErrorKind::Interrupted
                                    | ErrorKind::ConnectionAborted
                                    | ErrorKind::ConnectionReset
                            ) => {}
                        Err(error) => return Err(SignInError::Io(error.to_string())),
                    }
                }
            }
            while let Ok(verdict) = verdicts.try_recv() {
                match verdict {
                    Verdict::Code { code, client_id } => {
                        return Ok(super::SignInCode {
                            code,
                            redirect_uri,
                            verifier: pkce.verifier,
                            client_id,
                        });
                    }
                    Verdict::Denied(reason) => return Err(SignInError::Denied(reason)),
                    Verdict::BadRequest { wrong_state: true } => saw_wrong_state = true,
                    Verdict::BadRequest { wrong_state: false } | Verdict::NotFound => {}
                }
            }
            if Instant::now() >= deadline {
                return Err(if saw_wrong_state {
                    SignInError::StateMismatch
                } else {
                    SignInError::TimedOut
                });
            }
            thread::sleep(POLL_INTERVAL);
        }
    }
}

/// Binds the listeners for `host` on one free port: 127.0.0.1 for both, and
/// ::1 on the same port for `localhost` where the computer has IPv6
/// loopback (a browser may try either address for `localhost`).
fn bind(host: CallbackHost) -> std::io::Result<Vec<TcpListener>> {
    let ipv4 = || TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)));
    if host == CallbackHost::Ip127 {
        return Ok(vec![ipv4()?]);
    }
    let mut first = ipv4()?;
    for _ in 1..BIND_ATTEMPTS {
        let port = first.local_addr()?.port();
        match TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, port))) {
            Ok(second) => return Ok(vec![first, second]),
            // The port is taken on ::1: try another one.
            Err(error) if error.kind() == ErrorKind::AddrInUse => first = ipv4()?,
            // No IPv6 loopback here: 127.0.0.1 alone.
            Err(_) => return Ok(vec![first]),
        }
    }
    Ok(vec![first])
}

/// What every connection's thread needs to judge and answer its request.
struct Expected {
    path: &'static str,
    state: String,
    page: CallbackPage,
}

/// Reads one request, answers it and reports what it meant.
fn answer(mut stream: TcpStream, expected: &Expected, verdicts: &mpsc::Sender<Verdict>) {
    // A listener that does not block can hand out connections that do not
    // block either (BSD, macOS).
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(CONNECTION_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CONNECTION_TIMEOUT));
    let Some((method, target)) = read_request_line(&mut stream) else {
        return;
    };
    if method != "GET" {
        respond(
            &mut stream,
            "405 Method Not Allowed",
            &page_html("Method not allowed"),
        );
        return;
    }
    let verdict = judge(&target, expected.path, &expected.state);
    let (status, text) = match &verdict {
        Verdict::NotFound => ("404 Not Found", "Not found"),
        Verdict::BadRequest { .. } => ("400 Bad Request", expected.page.failure.as_str()),
        Verdict::Code { .. } => ("200 OK", expected.page.success.as_str()),
        Verdict::Denied(_) => ("200 OK", expected.page.failure.as_str()),
    };
    respond(&mut stream, status, &page_html(text));
    let _ = verdicts.send(verdict);
}

/// The method and target of the request on `stream`, once its head is in.
fn read_request_line(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 2048];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => request.extend_from_slice(&buffer[..n]),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        if request.len() > MAX_REQUEST_BYTES {
            break;
        }
    }
    let text = String::from_utf8_lossy(&request);
    let (line, _) = text.split_once("\r\n")?;
    let mut parts = line.split(' ');
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    Some((method, target))
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

/// The most of a refusal's reason that is kept.
const MAX_DENIED_CHARS: usize = 300;

/// What one request to the listener means.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// Another path: 404, keep waiting.
    NotFound,
    /// The right path but no usable callback: 400, keep waiting.
    /// `wrong_state`: it carried a code or an error with another state.
    BadRequest { wrong_state: bool },
    /// The callback with the code.
    Code {
        code: String,
        client_id: Option<String>,
    },
    /// The provider refused.
    Denied(String),
}

/// Judges a request target (`/callback?code=…`) against the expected path
/// and state.
fn judge(target: &str, path: &str, state: &str) -> Verdict {
    if !target.starts_with('/') {
        return Verdict::NotFound;
    }
    let Ok(url) = reqwest::Url::parse(&format!("http://loopback{target}")) else {
        return Verdict::NotFound;
    };
    if url.path() != path {
        return Verdict::NotFound;
    }
    let (mut code, mut got_state, mut error, mut description, mut client_id) =
        (None, None, None, None, None);
    for (name, value) in url.query_pairs() {
        let value = Some(value.into_owned()).filter(|value| !value.is_empty());
        match name.as_ref() {
            "code" => code = value,
            "state" => got_state = value,
            "error" => error = value,
            "error_description" => description = value,
            "client_id" => client_id = value,
            _ => {}
        }
    }
    let denied = |error: String| {
        let reason = match &description {
            Some(description) => format!("{error}: {description}"),
            None => error,
        };
        Verdict::Denied(reason.chars().take(MAX_DENIED_CHARS).collect())
    };
    match got_state {
        Some(got) if got != state => Verdict::BadRequest {
            wrong_state: code.is_some() || error.is_some(),
        },
        Some(_) => match (error, code) {
            (Some(error), _) => denied(error),
            (None, Some(code)) => Verdict::Code { code, client_id },
            (None, None) => Verdict::BadRequest { wrong_state: false },
        },
        // OpenRouter leaves the state out when the user denies access.
        None => match error {
            Some(error) => denied(error),
            None => Verdict::BadRequest { wrong_state: false },
        },
    }
}

/// The HTML page for `text`.
fn page_html(text: &str) -> String {
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>DanyWhere</title></head>\
         <body style=\"font-family:-apple-system,system-ui,sans-serif;margin:3em;font-size:1.1em\"><p>{}</p></body></html>\n",
        escape_html(text)
    )
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::super::{SignInCode, SignInFlow};
    use super::*;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    const STATE: &str = "state-abc_123";

    struct TestFlow {
        host: CallbackHost,
        path: &'static str,
    }

    impl SignInFlow for TestFlow {
        fn callback(&self) -> CallbackSpec {
            CallbackSpec {
                host: self.host,
                path: self.path,
            }
        }

        fn authorize_url(&self, redirect_uri: &str, state: &str, pkce: &Pkce) -> String {
            format!(
                "https://provider.example/authorize?redirect_uri={redirect_uri}&state={state}&challenge={}",
                pkce.challenge
            )
        }
    }

    fn page() -> CallbackPage {
        CallbackPage {
            success: "Connected <DanyWhere> & ready".to_string(),
            failure: "Sign-in failed".to_string(),
        }
    }

    fn start(host: CallbackHost, path: &'static str) -> PendingSignIn {
        PendingSignIn::start(&TestFlow { host, path }, page()).expect("listener starts")
    }

    fn port_of(redirect_uri: &str) -> u16 {
        reqwest::Url::parse(redirect_uri)
            .expect("redirect uri parses")
            .port()
            .expect("redirect uri has a port")
    }

    /// The `state` the sign-in put into its authorization URL.
    fn state_of(pending: &PendingSignIn) -> String {
        let url = reqwest::Url::parse(pending.auth_url()).expect("auth url parses");
        url.query_pairs()
            .find(|(name, _)| name == "state")
            .map(|(_, value)| value.into_owned())
            .expect("auth url has a state")
    }

    /// Sends one GET as a browser would and returns the status and the body.
    fn browser_get(address: SocketAddr, target: &str) -> std::io::Result<(u16, String)> {
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: {address}\r\nAccept: text/html\r\n\r\n"
        )?;
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer)?;
        let answer = String::from_utf8_lossy(&answer).into_owned();
        let status = answer
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        let body = answer
            .split_once("\r\n\r\n")
            .map(|(_, body)| body.to_string())
            .unwrap_or_default();
        Ok((status, body))
    }

    fn v4(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn v6(port: u16) -> SocketAddr {
        SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port))
    }

    fn ipv6_loopback_available() -> bool {
        TcpListener::bind(v6(0)).is_ok()
    }

    /// Runs `wait` in a thread of its own, as the app does.
    fn wait_in_thread(
        pending: PendingSignIn,
        timeout: Duration,
    ) -> (
        Arc<AtomicBool>,
        thread::JoinHandle<Result<SignInCode, SignInError>>,
    ) {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let handle = thread::spawn(move || pending.wait(timeout, &flag));
        (cancel, handle)
    }

    // RFC 7636, appendix B.
    #[test]
    fn the_challenge_of_the_rfc_example_verifier_is_the_rfc_example_challenge() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(pkce.verifier, "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
    }

    #[test]
    fn a_new_pair_has_a_valid_verifier_and_its_s256_challenge() {
        let first = Pkce::new().unwrap();
        let second = Pkce::new().unwrap();
        // RFC 7636, 4.1: 43 to 128 unreserved characters.
        assert!(
            (43..=128).contains(&first.verifier.len()),
            "{}",
            first.verifier.len()
        );
        assert!(first
            .verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
        assert_eq!(
            first.challenge,
            Pkce::from_verifier(first.verifier.clone()).challenge
        );
        assert_eq!(first.challenge.len(), 43);
        assert_ne!(first.verifier, second.verifier);
    }

    #[test]
    fn a_state_is_random_and_url_safe() {
        let first = random_state().unwrap();
        let second = random_state().unwrap();
        assert!(first.len() >= 22, "{first}");
        assert!(first
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(first, second);
    }

    #[test]
    fn the_debug_text_of_a_pair_hides_the_verifier() {
        let pkce = Pkce::from_verifier("verifier-that-must-not-show".to_string());
        assert!(!format!("{pkce:?}").contains("verifier-that-must-not-show"));
    }

    #[test]
    fn a_callback_with_the_right_state_and_a_code_is_the_code() {
        assert_eq!(
            judge("/callback?code=abc&state=state-abc_123", "/callback", STATE),
            Verdict::Code {
                code: "abc".to_string(),
                client_id: None
            }
        );
    }

    #[test]
    fn the_code_is_percent_decoded() {
        assert_eq!(
            judge(
                "/callback?state=state-abc_123&code=a%2Bb%2Fc%3D",
                "/callback",
                STATE
            ),
            Verdict::Code {
                code: "a+b/c=".to_string(),
                client_id: None
            }
        );
    }

    #[test]
    fn a_client_id_in_the_callback_comes_along() {
        assert_eq!(
            judge(
                "/auth/callback?code=abc&scope=openid&state=state-abc_123&client_id=app_new42",
                "/auth/callback",
                STATE
            ),
            Verdict::Code {
                code: "abc".to_string(),
                client_id: Some("app_new42".to_string())
            }
        );
    }

    #[test]
    fn a_callback_with_another_state_is_refused_and_marked() {
        assert_eq!(
            judge("/callback?code=abc&state=other", "/callback", STATE),
            Verdict::BadRequest { wrong_state: true }
        );
    }

    #[test]
    fn a_callback_without_a_code_is_refused() {
        assert_eq!(
            judge("/callback?state=state-abc_123", "/callback", STATE),
            Verdict::BadRequest { wrong_state: false }
        );
        assert_eq!(
            judge("/callback?state=state-abc_123&code=", "/callback", STATE),
            Verdict::BadRequest { wrong_state: false }
        );
    }

    #[test]
    fn a_code_without_a_state_is_refused() {
        assert_eq!(
            judge("/callback?code=abc", "/callback", STATE),
            Verdict::BadRequest { wrong_state: false }
        );
    }

    #[test]
    fn an_error_with_the_right_state_is_a_refusal_with_its_description() {
        assert_eq!(
            judge(
                "/callback?error=access_denied&error_description=The+user+said+no&state=state-abc_123",
                "/callback",
                STATE
            ),
            Verdict::Denied("access_denied: The user said no".to_string())
        );
    }

    // OpenRouter: "It is not returned if the user denies access."
    #[test]
    fn an_error_without_a_state_is_a_refusal() {
        assert_eq!(
            judge("/callback?error=access_denied", "/callback", STATE),
            Verdict::Denied("access_denied".to_string())
        );
    }

    #[test]
    fn an_error_with_another_state_is_refused_and_marked() {
        assert_eq!(
            judge(
                "/callback?error=access_denied&state=other",
                "/callback",
                STATE
            ),
            Verdict::BadRequest { wrong_state: true }
        );
    }

    #[test]
    fn another_path_is_not_found() {
        assert_eq!(judge("/favicon.ico", "/callback", STATE), Verdict::NotFound);
        assert_eq!(
            judge("/other?code=abc&state=state-abc_123", "/callback", STATE),
            Verdict::NotFound
        );
        assert_eq!(
            judge("/?code=abc&state=state-abc_123", "/callback", STATE),
            Verdict::NotFound
        );
    }

    #[test]
    fn the_page_escapes_its_text_and_declares_utf8() {
        let html = page_html("Verbunden <script>&\"'");
        assert!(
            html.contains("Verbunden &lt;script&gt;&amp;&quot;&#39;"),
            "{html}"
        );
        assert!(!html.contains("<script>"), "{html}");
        assert!(
            html.to_ascii_lowercase().contains("charset=\"utf-8\""),
            "{html}"
        );
    }

    #[test]
    fn the_error_texts_say_what_happened() {
        assert_eq!(SignInError::Cancelled.to_string(), "sign-in cancelled");
        assert_eq!(SignInError::TimedOut.to_string(), "sign-in timed out");
        assert_eq!(
            SignInError::Denied("access_denied".to_string()).to_string(),
            "sign-in denied: access_denied"
        );
        assert_eq!(
            SignInError::StateMismatch.to_string(),
            "sign-in callback had the wrong state"
        );
        assert_eq!(
            SignInError::Io("boom".to_string()).to_string(),
            "sign-in listener failed: boom"
        );
    }

    #[test]
    fn the_authorization_url_comes_from_the_flow_with_the_listener_state_and_challenge() {
        let pending = start(CallbackHost::Ip127, "/auth/callback");
        let url = reqwest::Url::parse(pending.auth_url()).unwrap();
        let pairs: std::collections::HashMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(pairs["redirect_uri"], pending.redirect_uri());
        assert_eq!(pairs["state"].len(), random_state().unwrap().len());
        assert_eq!(pairs["challenge"].len(), 43);
        assert_eq!(
            pairs["challenge"],
            Pkce::from_verifier(pending.pkce.verifier.clone()).challenge
        );
    }

    #[test]
    fn localhost_names_localhost_and_answers_on_both_loopback_addresses() {
        let pending = start(CallbackHost::Localhost, "/callback");
        let redirect = pending.redirect_uri().to_string();
        let port = port_of(&redirect);
        assert_eq!(redirect, format!("http://localhost:{port}/callback"));
        let (cancel, handle) = wait_in_thread(pending, Duration::from_secs(10));
        let (status, _) = browser_get(v4(port), "/favicon.ico").expect("127.0.0.1 answers");
        assert_eq!(status, 404);
        if ipv6_loopback_available() {
            let (status, _) = browser_get(v6(port), "/favicon.ico").expect("::1 answers");
            assert_eq!(status, 404);
        }
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(handle.join().unwrap().unwrap_err(), SignInError::Cancelled);
    }

    #[test]
    fn ip127_names_127_0_0_1_and_binds_only_ipv4() {
        let pending = start(CallbackHost::Ip127, "/auth/callback");
        let redirect = pending.redirect_uri().to_string();
        let port = port_of(&redirect);
        assert_eq!(redirect, format!("http://127.0.0.1:{port}/auth/callback"));
        assert!(TcpStream::connect_timeout(&v4(port), Duration::from_secs(2)).is_ok());
        if ipv6_loopback_available() {
            assert!(
                TcpStream::connect_timeout(&v6(port), Duration::from_secs(2)).is_err(),
                "::1 must not answer"
            );
        }
    }

    #[test]
    fn the_right_callback_ends_the_wait_with_the_code_and_shows_the_success_page() {
        let pending = start(CallbackHost::Localhost, "/callback");
        let port = port_of(pending.redirect_uri());
        let redirect = pending.redirect_uri().to_string();
        let verifier = pending.pkce.verifier.clone();
        let state = state_of(&pending);
        let (_cancel, handle) = wait_in_thread(pending, Duration::from_secs(10));

        let (status, body) =
            browser_get(v4(port), &format!("/callback?code=the-code&state={state}")).unwrap();
        assert_eq!(status, 200);
        assert!(
            body.contains("Connected &lt;DanyWhere&gt; &amp; ready"),
            "{body}"
        );

        let code = handle.join().unwrap().expect("a code");
        assert_eq!(code.code, "the-code");
        assert_eq!(code.redirect_uri, redirect);
        assert_eq!(code.verifier, verifier);
        assert_eq!(code.client_id, None);
    }

    #[test]
    fn a_client_id_of_the_callback_reaches_the_code() {
        let pending = start(CallbackHost::Ip127, "/auth/callback");
        let port = port_of(pending.redirect_uri());
        let state = state_of(&pending);
        let (_cancel, handle) = wait_in_thread(pending, Duration::from_secs(10));
        let (status, _) = browser_get(
            v4(port),
            &format!("/auth/callback?code=c1&scope=openid+email&state={state}&client_id=app_X1"),
        )
        .unwrap();
        assert_eq!(status, 200);
        let code = handle.join().unwrap().expect("a code");
        assert_eq!(code.client_id.as_deref(), Some("app_X1"));
    }

    #[test]
    fn wrong_requests_are_answered_and_do_not_end_the_wait() {
        let pending = start(CallbackHost::Localhost, "/callback");
        let port = port_of(pending.redirect_uri());
        let state = state_of(&pending);
        let (_cancel, handle) = wait_in_thread(pending, Duration::from_secs(10));

        let (status, body) = browser_get(v4(port), "/callback?code=x&state=wrong").unwrap();
        assert_eq!(status, 400);
        assert!(body.contains("Sign-in failed"), "{body}");
        let (status, _) = browser_get(v4(port), &format!("/callback?state={state}")).unwrap();
        assert_eq!(status, 400);
        let (status, _) =
            browser_get(v4(port), &format!("/elsewhere?code=x&state={state}")).unwrap();
        assert_eq!(status, 404);
        assert!(!handle.is_finished(), "the wait must go on");

        let (status, _) =
            browser_get(v4(port), &format!("/callback?code=real&state={state}")).unwrap();
        assert_eq!(status, 200);
        assert_eq!(handle.join().unwrap().unwrap().code, "real");
    }

    #[test]
    fn an_error_with_the_right_state_ends_the_wait_as_denied() {
        let pending = start(CallbackHost::Localhost, "/callback");
        let port = port_of(pending.redirect_uri());
        let state = state_of(&pending);
        let (_cancel, handle) = wait_in_thread(pending, Duration::from_secs(10));
        let (status, body) = browser_get(
            v4(port),
            &format!("/callback?error=access_denied&state={state}"),
        )
        .unwrap();
        assert_eq!(status, 200);
        assert!(body.contains("Sign-in failed"), "{body}");
        assert_eq!(
            handle.join().unwrap().unwrap_err(),
            SignInError::Denied("access_denied".to_string())
        );
    }

    #[test]
    fn the_listener_closes_after_the_first_valid_callback() {
        let pending = start(CallbackHost::Localhost, "/callback");
        let port = port_of(pending.redirect_uri());
        let state = state_of(&pending);
        let (_cancel, handle) = wait_in_thread(pending, Duration::from_secs(10));
        let target = format!("/callback?code=first&state={state}");
        assert_eq!(browser_get(v4(port), &target).unwrap().0, 200);
        assert_eq!(handle.join().unwrap().unwrap().code, "first");
        assert!(
            browser_get(v4(port), &format!("/callback?code=second&state={state}")).is_err(),
            "a second callback must find nobody listening"
        );
    }

    #[test]
    fn an_idle_connection_does_not_hold_up_the_callback() {
        let pending = start(CallbackHost::Localhost, "/callback");
        let port = port_of(pending.redirect_uri());
        let state = state_of(&pending);
        let (_cancel, handle) = wait_in_thread(pending, Duration::from_secs(10));
        // A browser's speculative connection that never sends a request.
        let _idle = TcpStream::connect(v4(port)).unwrap();
        let started = Instant::now();
        let (status, _) =
            browser_get(v4(port), &format!("/callback?code=c&state={state}")).unwrap();
        assert_eq!(status, 200);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(handle.join().unwrap().unwrap().code, "c");
    }

    #[test]
    fn cancelling_ends_the_wait() {
        let pending = start(CallbackHost::Localhost, "/callback");
        let (cancel, handle) = wait_in_thread(pending, Duration::from_secs(30));
        thread::sleep(Duration::from_millis(100));
        let started = Instant::now();
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(handle.join().unwrap().unwrap_err(), SignInError::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_wait_ends_at_its_time_limit() {
        let pending = start(CallbackHost::Localhost, "/callback");
        let started = Instant::now();
        let (_cancel, handle) = wait_in_thread(pending, Duration::from_millis(300));
        assert_eq!(handle.join().unwrap().unwrap_err(), SignInError::TimedOut);
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(300), "{elapsed:?}");
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    }

    #[test]
    fn a_wrong_state_before_the_time_limit_is_reported_as_such() {
        let pending = start(CallbackHost::Localhost, "/callback");
        let port = port_of(pending.redirect_uri());
        let (_cancel, handle) = wait_in_thread(pending, Duration::from_millis(800));
        assert_eq!(
            browser_get(v4(port), "/callback?code=x&state=stale")
                .unwrap()
                .0,
            400
        );
        assert_eq!(
            handle.join().unwrap().unwrap_err(),
            SignInError::StateMismatch
        );
    }

    #[test]
    fn the_debug_text_of_a_pending_sign_in_hides_the_verifier_and_the_state() {
        let pending = start(CallbackHost::Localhost, "/callback");
        let text = format!("{pending:?}");
        assert!(!text.contains(&pending.pkce.verifier), "{text}");
        assert!(!text.contains(&pending.state), "{text}");
    }
}
