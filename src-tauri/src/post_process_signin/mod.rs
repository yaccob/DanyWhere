//! Signing in to a post-processing provider through the browser: a loopback
//! listener with PKCE (RFC 8252, RFC 7636) that waits for the provider's
//! callback, and the providers that use it (OpenRouter here; ChatGPT joins
//! as `chatgpt`).
//!
//! The modules use only `std` and crates the app already has, and no
//! `crate::` paths, so they can be tested in a crate of their own; only
//! `commands`, which ties them to the app, uses the app's modules.

pub mod chatgpt;
pub mod commands;
pub mod loopback;
pub mod openrouter;

/// One way to sign in through the browser. Implemented by OpenRouter and
/// ChatGPT.
pub trait SignInFlow: Send + Sync {
    /// Where the loopback listener binds and which path the provider calls back on.
    fn callback(&self) -> loopback::CallbackSpec;
    /// The URL the browser opens; `redirect_uri` comes from the bound listener.
    fn authorize_url(&self, redirect_uri: &str, state: &str, pkce: &loopback::Pkce) -> String;
}

/// What a finished sign-in hands back to the caller, who exchanges `code`
/// for a key or tokens and stores the result. Its `Debug` hides the code and
/// the verifier.
pub struct SignInCode {
    pub code: String,
    pub redirect_uri: String,
    pub verifier: String,
    /// The callback's `client_id` query parameter, when it has one: ChatGPT
    /// sends the newly issued client id there on a first registration;
    /// OpenRouter sends none.
    pub client_id: Option<String>,
}

impl std::fmt::Debug for SignInCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SignInCode")
            .field("code", &"<hidden>")
            .field("redirect_uri", &self.redirect_uri)
            .field("verifier", &"<hidden>")
            .field("client_id", &self.client_id)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debug_text_of_a_code_hides_the_code_and_the_verifier() {
        let code = SignInCode {
            code: "secret-code-123".to_string(),
            redirect_uri: "http://localhost:4711/callback".to_string(),
            verifier: "secret-verifier-456".to_string(),
            client_id: Some("app_client".to_string()),
        };
        let text = format!("{code:?}");
        assert!(!text.contains("secret-code-123"), "{text}");
        assert!(!text.contains("secret-verifier-456"), "{text}");
        assert!(text.contains("http://localhost:4711/callback"), "{text}");
        assert!(text.contains("app_client"), "{text}");
    }
}
