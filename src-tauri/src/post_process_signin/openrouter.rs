//! Connecting an OpenRouter account with OAuth PKCE
//! (<https://openrouter.ai/docs/guides/overview/auth/oauth>): the browser
//! opens `/auth`, the callback brings a code, `POST /api/v1/auth/keys` turns
//! it into a key with a credit limit. `GET /api/v1/key` says whether the
//! account is still on the free tier, and the model list gives the free
//! models to fall back on.
//!
//! Every request takes the `base` (`DEFAULT_BASE` in the app) so that tests
//! can point it at a fake server.

use super::loopback::{CallbackHost, CallbackSpec, Pkce};
use serde_json::{json, Map, Value};

/// OpenRouter itself.
pub const DEFAULT_BASE: &str = "https://openrouter.ai";
/// The page the browser opens.
pub const AUTHORIZE_URL: &str = "https://openrouter.ai/auth";
/// The credit limit DanyWhere asks for on a new key, in US dollars.
pub const DEFAULT_CREDIT_LIMIT_USD: u32 = 10;
/// The name of the new key in the user's OpenRouter account.
pub const DEFAULT_KEY_LABEL: &str = "DanyWhere";
/// The model with credit: Claude Haiku 5.5, sent with
/// `provider.data_collection = "deny"`.
pub const PAID_MODEL: &str = "anthropic/claude-haiku-5.5";
/// OpenRouter refuses a longer `models` list: "'models' array must have 3
/// items or fewer." (HTTP 400, tried 2026-11-26).
pub const MAX_FALLBACK_MODELS: usize = 3;
/// Free models to try first, in this order, where the model list still has
/// them. Tried 2026-11-26: Nemotron 3.5 Super answered every request (13 of
/// 13, and the fallback and schema probes), the Gemma models were busy
/// (HTTP 429 upstream), Ling 3.2 Flash answered with a schema.
pub const FREE_MODEL_PREFERENCE: &[&str] = &[
    "nvidia/nemotron-3.5-super-vl:free",
    "google/gemma-4-31b-it:free",
    "inclusionai/ling-3.2-flash:free",
    "google/gemma-4-26b-a4b-it:free",
];
/// The smallest context a free model needs to take a long dictation with
/// its prompt.
pub const MIN_FREE_CONTEXT: u64 = 32_000;

/// The start of `llm_client`'s error text for an error answer
/// (`llm_client::STATUS_ERROR_PREFIX`), so that `classify_llm_error` reads
/// these errors too.
pub const STATUS_ERROR_PREFIX: &str = "API request failed with status ";

/// OpenRouter's sign-in: a key with a credit limit and a label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRouterFlow {
    /// `limit` of the new key in US dollars; `None` asks for no limit.
    pub credit_limit_usd: Option<u32>,
    /// `key_label`; empty leaves it to OpenRouter.
    pub key_label: String,
}

impl OpenRouterFlow {
    /// DanyWhere's choice: 10 US dollars, labelled "DanyWhere".
    pub fn danywhere() -> Self {
        OpenRouterFlow {
            credit_limit_usd: Some(DEFAULT_CREDIT_LIMIT_USD),
            key_label: DEFAULT_KEY_LABEL.to_string(),
        }
    }
}

impl super::SignInFlow for OpenRouterFlow {
    /// `localhost` on `/callback`, as in the trial of 2026-11-26 (OpenRouter
    /// takes localhost on any port).
    fn callback(&self) -> CallbackSpec {
        CallbackSpec {
            host: CallbackHost::Localhost,
            path: "/callback",
        }
    }

    fn authorize_url(&self, redirect_uri: &str, state: &str, pkce: &Pkce) -> String {
        let mut params = vec![
            ("callback_url", redirect_uri.to_string()),
            ("code_challenge", pkce.challenge.clone()),
            ("code_challenge_method", "S256".to_string()),
            ("state", state.to_string()),
        ];
        if let Some(limit) = self.credit_limit_usd {
            params.push(("limit", limit.to_string()));
        }
        if !self.key_label.is_empty() {
            params.push(("key_label", self.key_label.clone()));
        }
        match reqwest::Url::parse_with_params(AUTHORIZE_URL, &params) {
            Ok(url) => url.into(),
            // AUTHORIZE_URL is a valid URL; this cannot happen.
            Err(_) => AUTHORIZE_URL.to_string(),
        }
    }
}

/// Exchanges the callback's code for the user's new key
/// (`POST {base}/api/v1/auth/keys`).
pub async fn exchange_code(
    client: &reqwest::Client,
    base: &str,
    code: &super::SignInCode,
) -> Result<String, String> {
    let response = client
        .post(format!("{}/api/v1/auth/keys", base.trim_end_matches('/')))
        .json(&json!({
            "code": code.code,
            "code_verifier": code.verifier,
            "code_challenge_method": "S256",
        }))
        .send()
        .await
        .map_err(|error| transport_error("Failed to exchange the OpenRouter code", &error))?;
    let answer = json_answer(response).await?;
    match answer.get("key").and_then(Value::as_str) {
        Some(key) if !key.is_empty() => Ok(key.to_string()),
        _ => Err("OpenRouter answered the code exchange with no key".to_string()),
    }
}

/// The JSON of a successful answer, or the error in `llm_client`'s format.
async fn json_answer(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error response".to_string());
        return Err(format!("{STATUS_ERROR_PREFIX}{status}: {body}"));
    }
    response
        .json::<Value>()
        .await
        .map_err(|error| transport_error("Failed to read OpenRouter's answer", &error))
}

/// A failed request in the format of `llm_client::report_reqwest_error`
/// ("… (kind: connect, url: …)"), which `classify_llm_error` reads; the URL
/// without its query, and never a body.
fn transport_error(context: &str, error: &reqwest::Error) -> String {
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
    if error.is_body() {
        kinds.push("body");
    }
    if error.is_decode() {
        kinds.push("decode");
    }
    if kinds.is_empty() {
        kinds.push("unknown");
    }
    let url = error
        .url()
        .map(|url| {
            let mut url = url.clone();
            url.set_query(None);
            url.set_fragment(None);
            format!(", url: {url}")
        })
        .unwrap_or_default();
    let message = format!("{context} (kind: {}{url})", kinds.join(", "));
    log::error!("{message}");
    message
}

/// What `GET /api/v1/key` says about a key.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct KeyInfo {
    /// `is_free_tier`: the account never bought credits.
    pub free_tier: bool,
    /// `limit`: the key's credit limit in US dollars, `None` without one.
    pub limit_usd: Option<f64>,
    /// `usage`: what the key has used, in US dollars.
    pub usage_usd: f64,
    /// `label`. OpenRouter fills it with a shortened form of the key
    /// ("sk-or-v1-abc...xyz"), not with the `key_label` of the sign-in.
    pub label: Option<String>,
    /// `limit_remaining`: what is left of the limit, `None` without one.
    #[serde(default)]
    pub limit_remaining_usd: Option<f64>,
    /// `free_model_daily_requests.remaining`: free requests left today (UTC).
    #[serde(default)]
    pub free_requests_remaining: Option<u32>,
}

/// Reads what OpenRouter says about `key` (`GET {base}/api/v1/key`).
pub async fn key_info(client: &reqwest::Client, base: &str, key: &str) -> Result<KeyInfo, String> {
    let response = client
        .get(format!("{}/api/v1/key", base.trim_end_matches('/')))
        .bearer_auth(key)
        .send()
        .await
        .map_err(|error| transport_error("Failed to read the OpenRouter key", &error))?;
    let answer = json_answer(response).await?;
    let data = answer
        .get("data")
        .filter(|data| data.is_object())
        .ok_or_else(|| "OpenRouter's key info had no data".to_string())?;
    let free_tier = data
        .get("is_free_tier")
        .and_then(Value::as_bool)
        .ok_or_else(|| "OpenRouter's key info had no is_free_tier".to_string())?;
    Ok(KeyInfo {
        free_tier,
        limit_usd: data.get("limit").and_then(Value::as_f64),
        usage_usd: data.get("usage").and_then(Value::as_f64).unwrap_or(0.0),
        label: data
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_string),
        limit_remaining_usd: data.get("limit_remaining").and_then(Value::as_f64),
        free_requests_remaining: data
            .get("free_model_daily_requests")
            .and_then(|requests| requests.get("remaining"))
            .and_then(Value::as_u64)
            .and_then(|remaining| u32::try_from(remaining).ok()),
    })
}

/// OpenRouter's model list as it comes (`GET {base}/api/v1/models`, no key
/// needed), for [`choose_free_models`].
pub async fn list_models(client: &reqwest::Client, base: &str) -> Result<Value, String> {
    let response = client
        .get(format!("{}/api/v1/models", base.trim_end_matches('/')))
        .send()
        .await
        .map_err(|error| transport_error("Failed to read OpenRouter's model list", &error))?;
    json_answer(response).await
}

/// Free models to try in order, from `GET /api/v1/models`: the preference
/// list first, then the other `:free` models with a context of at least
/// [`MIN_FREE_CONTEXT`], models with an expiry date last; only text models
/// that cost nothing, and only models that take structured outputs when
/// `need_schema`. At most `max` (send at most [`MAX_FALLBACK_MODELS`]).
pub fn choose_free_models(models_json: &Value, need_schema: bool, max: usize) -> Vec<String> {
    let listed = match models_json {
        Value::Array(models) => models.as_slice(),
        Value::Object(_) => match models_json.get("data") {
            Some(Value::Array(models)) => models.as_slice(),
            _ => &[],
        },
        _ => &[],
    };
    // (id, expires) of each usable model, in the list's order.
    let usable: Vec<(&str, bool)> = listed
        .iter()
        .filter(|model| is_usable_free_model(model, need_schema))
        .filter_map(|model| {
            let id = model.get("id").and_then(Value::as_str)?;
            Some((id, has_expiry(model)))
        })
        .collect();
    let preferred = FREE_MODEL_PREFERENCE
        .iter()
        .copied()
        .filter(|wanted| usable.iter().any(|(id, _)| id == wanted));
    let lasting = usable
        .iter()
        .filter(|(_, expires)| !expires)
        .map(|(id, _)| *id);
    let expiring = usable
        .iter()
        .filter(|(_, expires)| *expires)
        .map(|(id, _)| *id);
    let mut chosen: Vec<String> = Vec::new();
    for candidate in preferred.chain(lasting).chain(expiring) {
        if chosen.len() >= max {
            break;
        }
        if !chosen.iter().any(|id| id == candidate) {
            chosen.push(candidate.to_string());
        }
    }
    chosen
}

/// A `:free` text model that costs nothing, takes a long dictation and,
/// when `need_schema`, structured outputs. OpenRouter's page on structured
/// outputs names `structured_outputs` as the parameter to look for;
/// `response_format` alone (Gemma on 2026-11-26) is JSON mode.
fn is_usable_free_model(model: &Value, need_schema: bool) -> bool {
    let Some(id) = model.get("id").and_then(Value::as_str) else {
        return false;
    };
    let costs_nothing = ["prompt", "completion"].iter().all(|part| {
        model
            .get("pricing")
            .and_then(|pricing| pricing.get(part))
            .and_then(Value::as_str)
            .and_then(|price| price.parse::<f64>().ok())
            == Some(0.0)
    });
    let handles_text = |direction: &str| {
        model
            .get("architecture")
            .and_then(|architecture| architecture.get(direction))
            .and_then(Value::as_array)
            .is_none_or(|modalities| modalities.iter().any(|m| m == "text"))
    };
    let context = model
        .get("context_length")
        .and_then(Value::as_u64)
        .or_else(|| {
            model
                .get("top_provider")
                .and_then(|top| top.get("context_length"))
                .and_then(Value::as_u64)
        })
        .unwrap_or(0);
    let takes_schema = || {
        model
            .get("supported_parameters")
            .and_then(Value::as_array)
            .is_some_and(|params| params.iter().any(|p| p == "structured_outputs"))
    };
    id.ends_with(":free")
        && costs_nothing
        && handles_text("input_modalities")
        && handles_text("output_modalities")
        && context >= MIN_FREE_CONTEXT
        && (!need_schema || takes_schema())
}

fn has_expiry(model: &Value) -> bool {
    model
        .get("expiration_date")
        .and_then(Value::as_str)
        .is_some_and(|date| !date.is_empty())
}

/// Extra fields for a chat completion request: `models` (the fallback list,
/// at most [`MAX_FALLBACK_MODELS`]) when free, `provider.data_collection =
/// "deny"` when paid. The request's `model` should be the list's first: a
/// request with `model` and three `models` is accepted (tried 2026-11-26),
/// and whether OpenRouter tries `model` first or only `models`, the list's
/// first then comes first.
pub fn request_extras(free_models: &[String], free_tier: bool) -> Map<String, Value> {
    let mut extras = Map::new();
    if free_tier {
        if !free_models.is_empty() {
            let models: Vec<Value> = free_models
                .iter()
                .take(MAX_FALLBACK_MODELS)
                .map(|model| Value::String(model.clone()))
                .collect();
            extras.insert("models".to_string(), Value::Array(models));
        }
    } else {
        extras.insert("provider".to_string(), json!({ "data_collection": "deny" }));
    }
    extras
}

#[cfg(test)]
mod tests {
    use super::super::{SignInCode, SignInFlow};
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const STORED_MODELS: &str = include_str!("testdata/openrouter_models.json");

    /// One request as the fake server saw it.
    #[derive(Debug, Clone)]
    struct Seen {
        request_line: String,
        headers: HashMap<String, String>,
        body: Value,
    }

    /// Answers one request per entry of `responses`, in turn, and keeps
    /// what each request was (pattern of `llm_client::serve_in_turn`).
    async fn serve_in_turn(
        responses: Vec<(&'static str, String)>,
    ) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
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
                        let head = &text[..end];
                        let headers: HashMap<String, String> = head
                            .lines()
                            .skip(1)
                            .filter_map(|line| line.split_once(':'))
                            .map(|(name, value)| {
                                (name.trim().to_ascii_lowercase(), value.trim().to_string())
                            })
                            .collect();
                        let length = headers
                            .get("content-length")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            let body =
                                serde_json::from_str(&text[end + 4..]).unwrap_or(Value::Null);
                            kept.lock().unwrap().push(Seen {
                                request_line: head.lines().next().unwrap_or("").to_string(),
                                headers,
                                body,
                            });
                            break;
                        }
                    }
                }
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{address}"), seen)
    }

    fn sign_in_code() -> SignInCode {
        SignInCode {
            code: "auth_code_abc123".to_string(),
            redirect_uri: "http://localhost:50538/callback".to_string(),
            verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string(),
            client_id: None,
        }
    }

    fn query_of(url: &str) -> HashMap<String, String> {
        reqwest::Url::parse(url)
            .expect("url parses")
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    fn stored_models() -> Value {
        serde_json::from_str(STORED_MODELS).expect("stored model list parses")
    }

    fn free_model(id: &str, context: u64, params: &[&str]) -> Value {
        json!({
            "id": id,
            "context_length": context,
            "pricing": {"prompt": "0", "completion": "0"},
            "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]},
            "supported_parameters": params,
            "expiration_date": null
        })
    }

    #[test]
    fn the_prefix_is_the_one_of_llm_client() {
        assert_eq!(STATUS_ERROR_PREFIX, "API request failed with status ");
    }

    #[test]
    fn danywhere_asks_for_ten_dollars_and_its_name() {
        let flow = OpenRouterFlow::danywhere();
        assert_eq!(flow.credit_limit_usd, Some(10));
        assert_eq!(flow.key_label, "DanyWhere");
    }

    // The trial of 2026-11-26 called back on http://localhost:<port>/callback.
    #[test]
    fn the_callback_goes_to_localhost_on_the_tried_path() {
        let spec = OpenRouterFlow::danywhere().callback();
        assert_eq!(spec.host, CallbackHost::Localhost);
        assert_eq!(spec.path, "/callback");
    }

    #[test]
    fn the_authorization_url_carries_challenge_state_limit_and_label() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string());
        let url = OpenRouterFlow::danywhere().authorize_url(
            "http://localhost:50538/callback",
            "st_1",
            &pkce,
        );
        assert!(url.starts_with("https://openrouter.ai/auth?"), "{url}");
        let query = query_of(&url);
        assert_eq!(query["callback_url"], "http://localhost:50538/callback");
        assert_eq!(
            query["code_challenge"],
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["state"], "st_1");
        assert_eq!(query["limit"], "10");
        assert_eq!(query["key_label"], "DanyWhere");
        assert_eq!(query.len(), 6, "{query:?}");
    }

    #[test]
    fn without_a_limit_or_a_label_the_url_leaves_them_out() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string());
        let flow = OpenRouterFlow {
            credit_limit_usd: None,
            key_label: String::new(),
        };
        let query = query_of(&flow.authorize_url("http://localhost:1/callback", "s", &pkce));
        assert!(!query.contains_key("limit"), "{query:?}");
        assert!(!query.contains_key("key_label"), "{query:?}");
        assert_eq!(query["code_challenge_method"], "S256");
    }

    #[test]
    fn a_label_with_spaces_and_symbols_is_encoded() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string());
        let flow = OpenRouterFlow {
            credit_limit_usd: Some(25),
            key_label: "DanyWhere & Co".to_string(),
        };
        let query = query_of(&flow.authorize_url("http://localhost:1/callback", "s", &pkce));
        assert_eq!(query["key_label"], "DanyWhere & Co");
        assert_eq!(query["limit"], "25");
    }

    #[tokio::test]
    async fn the_code_is_exchanged_with_its_verifier_for_the_key() {
        let (base, seen) = serve_in_turn(vec![(
            "200 OK",
            r#"{"key":"sk-or-v1-0e6f44","user_id":"user_2yOP"}"#.to_string(),
        )])
        .await;
        let key = exchange_code(&reqwest::Client::new(), &base, &sign_in_code())
            .await
            .expect("a key");
        assert_eq!(key, "sk-or-v1-0e6f44");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(
            seen[0].request_line.starts_with("POST /api/v1/auth/keys "),
            "{}",
            seen[0].request_line
        );
        assert_eq!(
            seen[0].body,
            json!({
                "code": "auth_code_abc123",
                "code_verifier": "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
                "code_challenge_method": "S256"
            })
        );
        assert!(seen[0].headers["content-type"].starts_with("application/json"));
        assert!(!seen[0].headers.contains_key("authorization"));
    }

    #[tokio::test]
    async fn a_refused_exchange_is_an_error_in_llm_client_format() {
        let body = r#"{"error":{"code":403,"message":"Invalid code or code_verifier"}}"#;
        let (base, _) = serve_in_turn(vec![("403 Forbidden", body.to_string())]).await;
        let error = exchange_code(&reqwest::Client::new(), &base, &sign_in_code())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            format!("API request failed with status 403 Forbidden: {body}")
        );
    }

    #[tokio::test]
    async fn an_exchange_answer_without_a_key_is_an_error() {
        let (base, _) =
            serve_in_turn(vec![("200 OK", r#"{"user_id":"user_2yOP"}"#.to_string())]).await;
        let error = exchange_code(&reqwest::Client::new(), &base, &sign_in_code())
            .await
            .unwrap_err();
        assert!(error.contains("no key"), "{error}");
        let (base, _) = serve_in_turn(vec![("200 OK", r#"{"key":""}"#.to_string())]).await;
        assert!(
            exchange_code(&reqwest::Client::new(), &base, &sign_in_code())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn an_unreachable_openrouter_is_a_connect_error() {
        // A port nobody listens on.
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let error = exchange_code(
            &reqwest::Client::new(),
            &format!("http://{address}"),
            &sign_in_code(),
        )
        .await
        .unwrap_err();
        assert!(error.contains("(kind: connect"), "{error}");
        assert!(!error.contains("auth_code_abc123"), "{error}");
    }

    /// The shape `GET /api/v1/key` answered with on 2026-11-26 (values made up).
    const KEY_FREE: &str = r#"{"data":{"label":"sk-or-v1-abc...xyz","limit":10,"limit_remaining":9.75,"limit_reset":null,"usage":0.25,"usage_daily":0,"usage_weekly":0,"usage_monthly":0,"byok_usage":0,"byok_usage_daily":0,"byok_usage_weekly":0,"byok_usage_monthly":0,"is_free_tier":true,"is_management_key":false,"is_provisioning_key":false,"include_byok_in_limit":false,"budgets":[],"effective_budget":null,"creator_user_id":"user_x","organization_id":null,"workspace_id":"ws_x","allowed_data_regions":["global"],"free_model_daily_requests":{"used":28,"limit":50,"remaining":22},"rate_limit":{"requests":-1,"interval":"10s","note":"This field is deprecated and safe to ignore."},"expires_at":null}}"#;

    #[tokio::test]
    async fn the_key_info_says_free_tier_limit_usage_and_what_is_left() {
        let (base, seen) = serve_in_turn(vec![("200 OK", KEY_FREE.to_string())]).await;
        let info = key_info(&reqwest::Client::new(), &base, "sk-or-v1-secret")
            .await
            .expect("key info");
        assert_eq!(
            info,
            KeyInfo {
                free_tier: true,
                limit_usd: Some(10.0),
                usage_usd: 0.25,
                label: Some("sk-or-v1-abc...xyz".to_string()),
                limit_remaining_usd: Some(9.75),
                free_requests_remaining: Some(22),
            }
        );
        let seen = seen.lock().unwrap();
        assert!(
            seen[0].request_line.starts_with("GET /api/v1/key "),
            "{}",
            seen[0].request_line
        );
        assert_eq!(seen[0].headers["authorization"], "Bearer sk-or-v1-secret");
    }

    #[tokio::test]
    async fn a_key_with_credit_and_no_limit_has_none() {
        let body = r#"{"data":{"label":"sk-or-v1-abc...xyz","limit":null,"limit_remaining":null,"usage":3.5,"is_free_tier":false}}"#;
        let (base, _) = serve_in_turn(vec![("200 OK", body.to_string())]).await;
        let info = key_info(&reqwest::Client::new(), &base, "k").await.unwrap();
        assert!(!info.free_tier);
        assert_eq!(info.limit_usd, None);
        assert_eq!(info.limit_remaining_usd, None);
        assert_eq!(info.usage_usd, 3.5);
        assert_eq!(info.free_requests_remaining, None);
    }

    #[tokio::test]
    async fn a_refused_key_is_an_error_in_llm_client_format_without_the_key() {
        let body = r#"{"error":{"code":401,"message":"User not found."}}"#;
        let (base, _) = serve_in_turn(vec![("401 Unauthorized", body.to_string())]).await;
        let error = key_info(&reqwest::Client::new(), &base, "sk-or-v1-secret")
            .await
            .unwrap_err();
        assert_eq!(
            error,
            format!("API request failed with status 401 Unauthorized: {body}")
        );
        assert!(!error.contains("sk-or-v1-secret"));
    }

    #[tokio::test]
    async fn a_key_info_without_data_is_an_error() {
        let (base, _) = serve_in_turn(vec![("200 OK", r#"{"nothing":1}"#.to_string())]).await;
        assert!(key_info(&reqwest::Client::new(), &base, "k").await.is_err());
    }

    // The stored list: GET /api/v1/models of 2026-11-26, cut down to the
    // free models, Claude Haiku 5.5, GPT-4o mini and openrouter/free.
    #[test]
    fn with_a_schema_the_stored_list_gives_preferred_models_that_take_structured_outputs() {
        assert_eq!(
            choose_free_models(&stored_models(), true, MAX_FALLBACK_MODELS),
            vec![
                "nvidia/nemotron-3.5-super-vl:free",
                "inclusionai/ling-3.2-flash:free",
                "baidu/ernie-5.5-flash-preview:free",
            ]
        );
    }

    #[test]
    fn without_a_schema_the_whole_preference_list_comes_first() {
        assert_eq!(
            choose_free_models(&stored_models(), false, 5),
            vec![
                "nvidia/nemotron-3.5-super-vl:free",
                "google/gemma-4-31b-it:free",
                "inclusionai/ling-3.2-flash:free",
                "google/gemma-4-26b-a4b-it:free",
                "baidu/ernie-5.5-flash-preview:free",
            ]
        );
    }

    #[test]
    fn only_free_text_models_with_enough_context_are_chosen() {
        let mut costly = free_model("paid/model:free", 100_000, &[]);
        costly["pricing"]["prompt"] = json!("0.000001");
        let mut images = free_model("image/model:free", 100_000, &[]);
        images["architecture"]["output_modalities"] = json!(["image"]);
        let list = json!({"data": [
            free_model("small/context:free", 8_192, &[]),
            costly,
            images,
            free_model("openrouter/free", 200_000, &[]),
            free_model("no/suffix", 200_000, &[]),
            free_model("good/one:free", 32_000, &[]),
            free_model("good/two:free", 131_072, &["structured_outputs", "response_format"]),
        ]});
        assert_eq!(
            choose_free_models(&list, false, 10),
            vec!["good/one:free", "good/two:free"]
        );
        assert_eq!(choose_free_models(&list, true, 10), vec!["good/two:free"]);
    }

    // OpenRouter's structured outputs page: look for `structured_outputs`;
    // `response_format` alone (Gemma) is JSON mode, not a schema.
    #[test]
    fn response_format_alone_does_not_count_as_taking_a_schema() {
        let list = json!({"data": [
            free_model("json/mode:free", 100_000, &["response_format"]),
            free_model("schema/ok:free", 100_000, &["response_format", "structured_outputs"]),
        ]});
        assert_eq!(choose_free_models(&list, true, 3), vec!["schema/ok:free"]);
    }

    #[test]
    fn models_with_an_expiry_date_come_last() {
        let mut leaving = free_model("leaving/soon:free", 100_000, &[]);
        leaving["expiration_date"] = json!("2026-12-31");
        let list = json!({"data": [leaving, free_model("staying/here:free", 100_000, &[])]});
        assert_eq!(
            choose_free_models(&list, false, 3),
            vec!["staying/here:free", "leaving/soon:free"]
        );
    }

    #[test]
    fn the_choice_keeps_to_max_and_has_no_duplicates() {
        let list = json!({"data": [
            free_model("nvidia/nemotron-3.5-super-vl:free", 100_000, &[]),
            free_model("a/b:free", 100_000, &[]),
            free_model("a/b:free", 100_000, &[]),
            free_model("c/d:free", 100_000, &[]),
        ]});
        assert_eq!(
            choose_free_models(&list, false, 10),
            vec!["nvidia/nemotron-3.5-super-vl:free", "a/b:free", "c/d:free"]
        );
        assert_eq!(choose_free_models(&list, false, 2).len(), 2);
        assert!(choose_free_models(&list, false, 0).is_empty());
    }

    #[test]
    fn a_list_without_models_gives_none() {
        assert!(choose_free_models(&json!({}), false, 3).is_empty());
        assert!(choose_free_models(&json!({"data": "x"}), false, 3).is_empty());
        assert!(choose_free_models(&json!([1, 2]), false, 3).is_empty());
    }

    #[test]
    fn a_bare_array_of_models_is_read_too() {
        let list = json!([free_model("a/b:free", 100_000, &[])]);
        assert_eq!(choose_free_models(&list, false, 3), vec!["a/b:free"]);
    }

    #[test]
    fn free_requests_fall_back_through_at_most_three_models() {
        let models: Vec<String> = ["a:free", "b:free", "c:free", "d:free"]
            .iter()
            .map(|id| id.to_string())
            .collect();
        let extras = request_extras(&models, true);
        assert_eq!(
            Value::Object(extras),
            json!({"models": ["a:free", "b:free", "c:free"]})
        );
    }

    #[test]
    fn free_requests_without_a_list_add_nothing() {
        assert!(request_extras(&[], true).is_empty());
    }

    #[test]
    fn paid_requests_only_go_to_providers_that_do_not_collect_data() {
        let models = vec!["a:free".to_string()];
        assert_eq!(
            Value::Object(request_extras(&models, false)),
            json!({"provider": {"data_collection": "deny"}})
        );
    }

    #[test]
    fn the_paid_model_is_in_the_stored_list_costs_money_and_takes_structured_outputs() {
        let list = stored_models();
        let haiku = list["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|model| model["id"] == PAID_MODEL)
            .expect("the paid model is listed");
        assert_ne!(haiku["pricing"]["prompt"], "0");
        assert!(haiku["supported_parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "structured_outputs"));
    }
}
